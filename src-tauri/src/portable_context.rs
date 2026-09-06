//! Explicit context for the self-contained OpenClaw preview bundle.
//!
//! A green executable alone is not portable: the application must decide its
//! storage roots before wallet or runtime code asks for a home directory.  The
//! marker is deliberately opt-in so the normal installed U-King keeps its
//! existing semantics. `UKING_TEST_HOME` remains a test sandbox only.

use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const MARKER: &str = "portable.json";
const OWNER: &str = "u-king-openclaw-portable";

#[derive(Debug, Deserialize)]
struct Marker {
    schema_version: u32,
    owner: String,
    runtime_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortableContext {
    pub root: PathBuf,
}

impl PortableContext {
    pub fn uking_home(&self) -> PathBuf {
        self.root.join("U-King").join("data").join("uking")
    }
    pub fn openclaw_root(&self) -> PathBuf {
        self.root.join("U-King").join("OpenClaw")
    }
}

fn is_safe_root(root: &Path) -> bool {
    // The marker lives beside the executable. Reject a marker reached through
    // a reparse point instead of treating an arbitrary host directory as part
    // of the portable package.
    fs::symlink_metadata(root)
        .map(|m| m.is_dir() && !is_reparse_metadata(&m))
        .unwrap_or(false)
}

fn is_reparse_metadata(meta: &fs::Metadata) -> bool {
    if meta.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        return meta.file_attributes() & 0x0400 != 0;
    }
    #[cfg(not(windows))]
    false
}

/// `exists()` follows links and turns both a dangling link and an access
/// failure into `false`. That is unsuitable for the marker which decides
/// whether state may leave the portable package.
fn marker_is_present(marker: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(marker) {
        Ok(meta) if is_reparse_metadata(&meta) => Err("portable.json 不能是链接或重解析点".into()),
        Ok(_) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(format!("无法安全检查 portable.json: {err}")),
    }
}

pub fn from_root(root: PathBuf) -> Result<PortableContext, String> {
    if !is_safe_root(&root) {
        return Err("便携包根目录不存在或是重解析点".into());
    }
    let marker_path = root.join(MARKER);
    if !marker_is_present(&marker_path)? {
        return Err("便携包缺少 portable.json 标记".into());
    }
    let marker: Marker = serde_json::from_slice(
        &fs::read(&marker_path).map_err(|e| format!("无法读取 portable.json: {e}"))?,
    )
    .map_err(|_| "portable.json 无效")?;
    if marker.schema_version != 1 || marker.owner != OWNER || marker.runtime_id != "openclaw2" {
        return Err("portable.json 不属于受管的 OpenClaw 便携包".into());
    }
    Ok(PortableContext { root })
}

/// Returns the portable context only when the executable sits beside our
/// marker. No environment variable can enable production portable mode.
fn detected_current() -> &'static Result<Option<PortableContext>, String> {
    static DETECTED: OnceLock<Result<Option<PortableContext>, String>> = OnceLock::new();
    DETECTED.get_or_init(|| {
        let root = std::env::current_exe()
            .map_err(|e| format!("无法定位当前程序: {e}"))?
            .parent()
            .ok_or("当前程序没有父目录")?
            .to_path_buf();
        if !marker_is_present(&root.join(MARKER))? {
            return Ok(None);
        }
        from_root(root).map(Some)
    })
}

/// The executable's mode is fixed for its whole lifetime. In particular, a
/// marker removed after startup must not make later writes fall back to host
/// storage.
pub fn current() -> Option<PortableContext> {
    detected_current().as_ref().ok().and_then(Clone::clone)
}

/// Startup gate: an absent marker means the ordinary desktop product, while a
/// present but malformed marker is an unsafe partial portable package and must
/// never fall through to host storage.
pub fn validate_current_executable() -> Result<(), String> {
    detected_current()
        .as_ref()
        .map(|_| ())
        .map_err(Clone::clone)
}

pub fn uking_home() -> Option<PathBuf> {
    current().map(|ctx| ctx.uking_home())
}
pub fn openclaw_root() -> Option<PathBuf> {
    current().map(|ctx| ctx.openclaw_root())
}

/// The preview is an appliance, not a second route into the desktop product.
/// Keep this list next to the marker contract so CLI, MCP and WebView callers
/// all receive the same fail-closed answer before an unrelated Action can
/// inspect or alter host state.
pub fn action_allowed(id: &str) -> bool {
    matches!(
        id,
        "runtime.openclaw2.inspect"
            | "runtime.openclaw2.prepare"
            | "runtime.openclaw2.preflight"
            | "runtime.openclaw2.launch"
            | "runtime.openclaw2.open_dashboard"
            | "runtime.openclaw2.configure_model_no_probe"
            | "runtime.openclaw2.configure_model"
            | "runtime.openclaw2.stop"
            | "runtime.device.wallet.recharge"
            | "runtime.device.wallet.status"
            | "runtime.device.wallet.copy_backup"
            | "runtime.device.key_adopt"
            | "runtime.device.key_rotate"
    )
}

/// Check every existing component beneath the marker root before a portable
/// writer creates or follows it. Windows junctions are reparse points too;
/// accepting one would redirect package state into a host directory.
pub fn ensure_owned_path(path: &Path) -> Result<(), String> {
    let context = current().ok_or("当前不是受管便携包")?;
    ensure_owned_path_from_root(&context.root, path)
}

/// Verify a prospective portable path without following any existing link.
/// `symlink_metadata` sees dangling links, while `exists()` would hide them.
pub fn ensure_owned_path_from_root(root: &Path, path: &Path) -> Result<(), String> {
    if !is_safe_root(root) {
        return Err("便携包根目录不存在或是重解析点".into());
    }
    let suffix = path
        .strip_prefix(root)
        .map_err(|_| "便携路径越出包根目录")?;
    let mut cursor = root.to_path_buf();
    check_component(&cursor)?;
    for component in suffix.components() {
        cursor.push(component);
        check_component(&cursor)?;
    }
    Ok(())
}

fn check_component(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(meta) if is_reparse_metadata(&meta) => Err(format!(
            "便携包受管路径包含链接或重解析点: {}",
            path.display()
        )),
        Ok(_) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(format!("无法安全检查便携路径 {}: {err}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_marker_resolves_only_package_relative_paths() {
        let root =
            std::env::temp_dir().join(format!("uking-portable-context-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join(MARKER),
            br#"{"schema_version":1,"owner":"u-king-openclaw-portable","runtime_id":"openclaw2"}"#,
        )
        .unwrap();
        let ctx = from_root(root.clone()).unwrap();
        assert_eq!(ctx.uking_home(), root.join("U-King/data/uking"));
        assert_eq!(ctx.openclaw_root(), root.join("U-King/OpenClaw"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn unknown_marker_fails_closed() {
        let root =
            std::env::temp_dir().join(format!("uking-portable-invalid-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join(MARKER),
            br#"{"schema_version":1,"owner":"other","runtime_id":"openclaw2"}"#,
        )
        .unwrap();
        assert!(from_root(root.clone()).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn wallet_actions_are_limited_to_the_managed_wallet_contract() {
        for id in [
            "runtime.device.wallet.recharge",
            "runtime.device.wallet.status",
            "runtime.device.wallet.copy_backup",
            "runtime.device.key_adopt",
            "runtime.device.key_rotate",
        ] {
            assert!(action_allowed(id), "便携钱包动作必须经过同一个白名单: {id}");
        }
        assert!(!action_allowed("runtime.device.wallet_reset_local"));
        assert!(!action_allowed("runtime.device.wallet.delete_server_wallet"));
    }

    #[cfg(windows)]
    #[test]
    fn dangling_marker_and_linked_wallet_path_fail_closed() {
        use std::os::windows::fs::{symlink_dir, symlink_file};

        let root =
            std::env::temp_dir().join(format!("uking-portable-links-{}", std::process::id()));
        let outside =
            std::env::temp_dir().join(format!("uking-portable-sentinel-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&outside).unwrap();
        let sentinel = outside.join("wallet-sentinel.txt");
        fs::write(&sentinel, b"host bytes must not change").unwrap();

        symlink_file(root.join("missing-marker.json"), root.join(MARKER)).unwrap();
        assert!(
            from_root(root.clone()).is_err(),
            "悬空 marker 不能退回桌面模式"
        );
        fs::remove_file(root.join(MARKER)).unwrap();
        fs::write(
            root.join(MARKER),
            br#"{"schema_version":1,"owner":"u-king-openclaw-portable","runtime_id":"openclaw2"}"#,
        )
        .unwrap();
        symlink_dir(&outside, root.join("U-King")).unwrap();
        assert!(
            ensure_owned_path_from_root(&root, &root.join("U-King/data/uking/device.json"))
                .is_err()
        );
        assert_eq!(fs::read(&sentinel).unwrap(), b"host bytes must not change");
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
    }
}
