//! Local-first creator canvas state.  The browser is deliberately a replaceable
//! surface: this module owns the project bytes and never trusts IndexedDB as a
//! source of truth.

pub(crate) mod component;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex, OnceLock,
};
use std::thread;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_CANVAS_BYTES: usize = 32 * 1024 * 1024;
const MAX_PROMPT_CHARS: usize = 8_000;
const ALLOWED_SIZES: &[&str] = &["1024x1024", "1024x1536", "1536x1024"];
const ALLOWED_QUALITIES: &[&str] = &["low", "medium", "high"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub schema: u32,
    pub id: String,
    pub title: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub state_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ImageTask {
    schema: u32,
    id: String,
    execution_id: String,
    request_fingerprint: String,
    status: String,
    prompt: String,
    /// Requested model is part of the idempotency fingerprint. `actual_model`
    /// is written only when a generator actually returns a file; do not claim
    /// that a fallback/fake/timeout used the requested model.
    model: String,
    #[serde(default)]
    actual_model: Option<String>,
    size: String,
    quality: Option<String>,
    created_at: i64,
    updated_at: i64,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ResourceIntegrityManifest {
    schema: u32,
    files: Vec<ResourceIntegrityEntry>,
}

#[derive(Debug, Clone, Deserialize)]
struct ResourceIntegrityEntry {
    path: String,
    sha256: String,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn uking_home() -> PathBuf {
    if let Ok(home) = std::env::var("UKING_TEST_HOME") {
        return PathBuf::from(home).join(".uking");
    }
    crate::installer::uking_home()
}

fn projects_root() -> PathBuf {
    uking_home().join("projects")
}

/// Resolve one already-created project below the canonical projects root.
/// `PathBuf::join` is not sufficient on Windows: a pre-existing junction below
/// `.uking/projects` could otherwise send image assets outside U-King state.
fn checked_project_dir(id: &str) -> Result<PathBuf, String> {
    if !valid_id(id) {
        return Err("invalid_input: project_id 格式不合法".into());
    }
    let root = projects_root();
    let root_canonical = root
        .canonicalize()
        .map_err(|_| "not_found: 创作项目根目录不存在".to_string())?;
    let candidate = root.join(format!("canvas-{id}"));
    let candidate_canonical = candidate
        .canonicalize()
        .map_err(|_| "not_found: 创作项目不存在".to_string())?;
    if !candidate_canonical.starts_with(&root_canonical) || !candidate_canonical.is_dir() {
        return Err("forbidden: 创作项目路径越界或指向重解析点".into());
    }
    Ok(candidate_canonical)
}

fn checked_project_child(project: &Path, name: &str) -> Result<PathBuf, String> {
    if !matches!(name, "assets" | "tasks") {
        return Err("invalid_input: 非法项目子目录".into());
    }
    let project = project
        .canonicalize()
        .map_err(|_| "not_found: 创作项目不存在".to_string())?;
    let child = project.join(name);
    if !child.exists() {
        fs::create_dir_all(&child).map_err(|e| format!("创建项目子目录失败: {e}"))?;
    }
    let child = child
        .canonicalize()
        .map_err(|_| "forbidden: 项目子目录无法解析".to_string())?;
    if !child.starts_with(&project) || !child.is_dir() {
        return Err("forbidden: 项目子目录越界或指向重解析点".into());
    }
    Ok(child)
}

/// Check the final asset directory before a task is journaled/submitted. It
/// proves both canonical containment (including junctions) and that the
/// project can actually receive the result, so a provider is never called for
/// an unwritable destination.
fn preflight_assets_writable(project: &Path) -> Result<(), String> {
    let assets = checked_project_child(project, "assets")?;
    let probe = assets.join(format!(".uking-write-probe-{}", fresh_id()));
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .map_err(|e| format!("not_writable: 创作项目素材目录不可写: {e}"))?;
    fs::remove_file(&probe).map_err(|e| format!("not_writable: 无法清理素材目录写入探针: {e}"))
}

fn project_dir(id: &str) -> Result<PathBuf, String> {
    if !valid_id(id) {
        return Err("invalid_input: project_id 格式不合法".into());
    }
    checked_project_dir(id)
}
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}
static ID_SEQUENCE: AtomicU64 = AtomicU64::new(0);
// ActionParity checks a version before calling the handler, but two handlers
// can pass that outer check at the same time. Keep the handler's own version
// comparison and file replacement together, otherwise the second handler can
// still become an undeclared last writer.
static PROJECT_WRITE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
// `execution_id` is a billing boundary, so checking its journal entry and
// creating the initial pending record must be one critical section. Without
// this, two concurrent ActionParity deliveries could both see an empty tasks/
// directory and submit twice before either reaches the provider.
static TASK_JOURNAL_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn task_journal_lock() -> &'static Mutex<()> {
    TASK_JOURNAL_LOCK.get_or_init(|| Mutex::new(()))
}

fn project_write_lock() -> &'static Mutex<()> {
    PROJECT_WRITE_LOCK.get_or_init(|| Mutex::new(()))
}

fn fresh_id() -> String {
    format!(
        "{:x}-{:x}-{:x}",
        now_ms(),
        std::process::id(),
        ID_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    )
}
fn canvas_path(dir: &Path) -> PathBuf {
    dir.join("canvas.json")
}
fn manifest_path(dir: &Path) -> PathBuf {
    dir.join("manifest.json")
}

/// Replace-by-rename keeps a power loss from turning a valid canvas into a
/// half-written JSON file.  The temp stays beside the final file so Windows
/// rename is atomic on the same volume.
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("invalid target")?;
    fs::create_dir_all(parent).map_err(|e| format!("创建项目目录失败: {e}"))?;
    let tmp = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name().and_then(|s| s.to_str()).unwrap_or("write"),
        now_ms()
    ));
    fs::write(&tmp, bytes).map_err(|e| format!("写临时项目文件失败: {e}"))?;
    replace_file(&tmp, path)
}

#[cfg(windows)]
fn replace_file(from: &Path, to: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" {
        fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32;
    }
    let wide = |p: &Path| {
        p.as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>()
    };
    let from = wide(from);
    let to = wide(to);
    // MOVEFILE_REPLACE_EXISTING: same-volume replacement, unlike a remove + rename window.
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 0x1) } == 0 {
        return Err(format!(
            "提交项目文件失败: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}
#[cfg(not(windows))]
fn replace_file(from: &Path, to: &Path) -> Result<(), String> {
    fs::rename(from, to).map_err(|e| format!("提交项目文件失败: {e}"))
}

fn read_manifest(dir: &Path) -> Result<Manifest, String> {
    let raw = fs::read_to_string(manifest_path(dir))
        .map_err(|_| "not_found: 创作项目不存在".to_string())?;
    serde_json::from_str(&raw).map_err(|e| format!("项目清单损坏: {e}"))
}
fn state_version(canvas: &str) -> String {
    crate::actions::version_of(canvas)
}

/// ActionParity's generic state hook needs one stable snapshot.  It includes
/// every project's manifest/canvas so a caller cannot read project A then
/// blindly overwrite after any project state changed.  Per-canvas versions are
/// retained in each manifest for diagnostics, but the action gate uses this
/// authoritative root version.
pub fn projects_state_version() -> String {
    let root = projects_root();
    let mut chunks = Vec::new();
    if let Ok(entries) = fs::read_dir(root) {
        let mut paths = entries.flatten().map(|e| e.path()).collect::<Vec<_>>();
        paths.sort();
        for path in paths {
            if !path.is_dir() {
                continue;
            }
            for name in ["manifest.json", "canvas.json"] {
                if let Ok(raw) = fs::read_to_string(path.join(name)) {
                    chunks.push(raw);
                }
            }
        }
    }
    crate::actions::version_of(&chunks.join("\n"))
}

pub fn create_project(title: Option<&str>) -> Result<Value, String> {
    let id = fresh_id();
    create_project_with_id(&id, title)
}

/// Read-only project picker data.  We derive every row from its canonical
/// manifest rather than trusting a browser cache or directory display name.
pub fn list_projects() -> Result<Value, String> {
    let _read = project_write_lock()
        .lock()
        .map_err(|_| "创作项目读锁损坏".to_string())?;
    let root = projects_root();
    let mut projects = Vec::new();
    if !root.exists() {
        return Ok(json!({ "projects": projects }));
    }
    for entry in fs::read_dir(&root)
        .map_err(|e| format!("读取创作项目失败: {e}"))?
        .flatten()
    {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Some(id) = name.strip_prefix("canvas-") else {
            continue;
        };
        let Ok(dir) = checked_project_dir(id) else {
            continue;
        };
        let Ok(manifest) = read_manifest(&dir) else {
            continue;
        };
        projects.push(json!({
            "id": manifest.id,
            "title": manifest.title,
            "updated_at": manifest.updated_at,
            "created_at": manifest.created_at,
        }));
    }
    projects.sort_by(|left, right| {
        right["updated_at"]
            .as_i64()
            .cmp(&left["updated_at"].as_i64())
            .then_with(|| left["title"].as_str().cmp(&right["title"].as_str()))
            .then_with(|| left["id"].as_str().cmp(&right["id"].as_str()))
    });
    Ok(json!({ "projects": projects }))
}

fn create_project_with_id(id: &str, title: Option<&str>) -> Result<Value, String> {
    let _write = project_write_lock()
        .lock()
        .map_err(|_| "创作项目写锁损坏".to_string())?;
    if !valid_id(id) {
        return Err("invalid_input: project_id 格式不合法".into());
    }
    let dir = projects_root().join(format!("canvas-{id}"));
    if dir.exists() {
        return Err("target_conflict: 创作项目已存在".into());
    }
    let canvas = "{\"children\":[]}";
    let now = now_ms();
    let manifest = Manifest {
        schema: 1,
        id: id.into(),
        title: title
            .unwrap_or("未命名画布")
            .trim()
            .chars()
            .take(120)
            .collect(),
        created_at: now,
        updated_at: now,
        state_version: state_version(canvas),
    };
    fs::create_dir_all(dir.join("assets")).map_err(|e| format!("创建素材目录失败: {e}"))?;
    fs::create_dir_all(dir.join("tasks")).map_err(|e| format!("创建任务目录失败: {e}"))?;
    let dir = checked_project_dir(id)?;
    atomic_write(&canvas_path(&dir), canvas.as_bytes())?;
    atomic_write(
        &manifest_path(&dir),
        &serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?,
    )?;
    Ok(
        json!({ "id": id, "title": manifest.title, "state_version": projects_state_version(), "canvas_state_version": manifest.state_version }),
    )
}

pub fn inspect_project(id: &str) -> Result<Value, String> {
    // A canvas document and its manifest are two separately atomically-replaced
    // files. The shared state lock makes inspect observe a matching pair rather
    // than the interval between the two replacements.
    let _read = project_write_lock()
        .lock()
        .map_err(|_| "创作项目读锁损坏".to_string())?;
    let dir = project_dir(id)?;
    let manifest = read_manifest(&dir)?;
    let canvas: Value = serde_json::from_str(
        &fs::read_to_string(canvas_path(&dir)).map_err(|e| format!("读画布失败: {e}"))?,
    )
    .map_err(|e| format!("画布损坏: {e}"))?;
    Ok(
        json!({ "id": manifest.id, "title": manifest.title, "canvas": canvas,
        "state_version": projects_state_version(), "canvas_state_version": manifest.state_version, "updated_at": manifest.updated_at }),
    )
}

pub fn save_project(id: &str, canvas: &Value, expected: Option<&str>) -> Result<Value, String> {
    let _write = project_write_lock()
        .lock()
        .map_err(|_| "创作项目写锁损坏".to_string())?;
    let dir = project_dir(id)?;
    let mut manifest = read_manifest(&dir)?;
    let expected = expected
        .filter(|s| !s.is_empty())
        .ok_or("invalid_input: 保存必须带 inspect 返回的 expected_state_version")?;
    let current = projects_state_version();
    if expected != current {
        return Err(format!(
            "conflict: 状态在你读到之后被改过（expected={expected} current={current}）"
        ));
    }
    let bytes = serde_json::to_vec(canvas).map_err(|e| format!("画布序列化失败: {e}"))?;
    if bytes.len() > MAX_CANVAS_BYTES {
        return Err("invalid_input: 画布超过 32MB 上限".into());
    }
    let raw = String::from_utf8(bytes).map_err(|_| "画布不是 UTF-8 JSON".to_string())?;
    manifest.updated_at = now_ms();
    manifest.state_version = state_version(&raw);
    atomic_write(&canvas_path(&dir), raw.as_bytes())?;
    atomic_write(
        &manifest_path(&dir),
        &serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?,
    )?;
    Ok(
        json!({ "id": id, "state_version": projects_state_version(), "canvas_state_version": manifest.state_version, "updated_at": manifest.updated_at }),
    )
}

pub fn validate_image_input(
    prompt: &str,
    model: &str,
    size: &str,
    quality: Option<&str>,
) -> Result<(), String> {
    if prompt.trim().is_empty() || prompt.chars().count() > MAX_PROMPT_CHARS {
        return Err("invalid_input: prompt 不能为空且最多 8000 字符".into());
    }
    if model != "gpt-image-2" {
        return Err("invalid_input: 首版只允许 gpt-image-2".into());
    }
    if !ALLOWED_SIZES.contains(&size) {
        return Err("invalid_input: size 不在白名单".into());
    }
    if let Some(q) = quality {
        if !ALLOWED_QUALITIES.contains(&q) {
            return Err("invalid_input: quality 不在白名单".into());
        }
    }
    Ok(())
}

#[derive(Debug)]
pub enum ImageSubmitPreparation {
    New { task_id: String },
    Existing(Value),
}

fn task_file_name(execution_id: &str) -> String {
    format!(
        "image-{}",
        crate::actions::version_of(execution_id).trim_start_matches("v1-")
    )
}
fn request_fingerprint(prompt: &str, model: &str, size: &str, quality: Option<&str>) -> String {
    crate::actions::version_of(&format!(
        "{prompt}\n{model}\n{size}\n{}",
        quality.unwrap_or("")
    ))
}
fn task_path(dir: &Path, task_id: &str) -> Result<PathBuf, String> {
    if !valid_id(task_id) {
        return Err("invalid_input: task_id 格式不合法".into());
    }
    Ok(checked_project_child(dir, "tasks")?.join(format!("{task_id}.json")))
}
fn find_task_by_id(task_id: &str) -> Option<(PathBuf, ImageTask)> {
    let entries = fs::read_dir(projects_root()).ok()?;
    for entry in entries.flatten() {
        let raw_dir = entry.path();
        let Some(name) = raw_dir.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Some(project_id) = name.strip_prefix("canvas-") else {
            continue;
        };
        // Never follow an arbitrary directory/junction returned by read_dir.
        // `checked_project_dir` canonicalizes the project below the root first.
        let Ok(dir) = checked_project_dir(project_id) else {
            continue;
        };
        let Ok(path) = task_path(&dir, task_id) else {
            continue;
        };
        if path.is_file() {
            if let Ok(task) = read_task(&path) {
                return Some((dir, task));
            }
        }
    }
    None
}
fn read_task(path: &Path) -> Result<ImageTask, String> {
    serde_json::from_str(
        &fs::read_to_string(path).map_err(|_| "not_found: 创作图片任务不存在".to_string())?,
    )
    .map_err(|e| format!("任务记录损坏: {e}"))
}
fn save_task(dir: &Path, task: &ImageTask) -> Result<(), String> {
    atomic_write(
        &task_path(dir, &task.id)?,
        &serde_json::to_vec_pretty(task).map_err(|e| e.to_string())?,
    )
}

fn existing_task_output(project_id: &str, task: ImageTask) -> Value {
    match task.status.as_str() {
        "completed" => task.result.unwrap_or_else(
            || json!({ "project_id": project_id, "task_id": task.id, "status": "completed" }),
        ),
        "pending-submit" | "pending-verify" => {
            json!({ "project_id": project_id, "task_id": task.id, "status": "pending-verify" })
        }
        "not_configured" => json!({
            "project_id": project_id,
            "task_id": task.id,
            "status": "not_configured",
            "error": task.error,
        }),
        "failed" => {
            json!({ "project_id": project_id, "task_id": task.id, "status": "failed", "error": task.error })
        }
        _ => json!({ "project_id": project_id, "task_id": task.id, "status": "pending-verify" }),
    }
}

/// The paid-operation journal is written *before* the provider call. On a
/// process crash we return pending-verify, never guess and submit again.
pub fn prepare_image_submit(
    id: &str,
    execution_id: &str,
    prompt: &str,
    model: &str,
    size: &str,
    quality: Option<&str>,
) -> Result<ImageSubmitPreparation, String> {
    let _journal = task_journal_lock()
        .lock()
        .map_err(|_| "图片任务幂等锁损坏".to_string())?;
    validate_image_input(prompt, model, size, quality)?;
    let dir = project_dir(id)?;
    let _ = read_manifest(&dir)?; // existence + readable project before any paid operation
    preflight_assets_writable(&dir)?;
    fs::create_dir_all(dir.join("tasks")).map_err(|e| format!("创建任务目录失败: {e}"))?;
    let task_id = task_file_name(execution_id);
    let path = task_path(&dir, &task_id)?;
    let fingerprint = request_fingerprint(prompt, model, size, quality);
    // execution_id is action-wide, not scoped to a project. A replay pointed at
    // another project or with changed parameters is a conflict, never a new bill.
    if let Some((found_dir, task)) = find_task_by_id(&task_id) {
        if found_dir != dir
            || task.execution_id != execution_id
            || task.request_fingerprint != fingerprint
        {
            return Err("conflict: 同一幂等键对应了不同的图片请求".into());
        }
        return Ok(ImageSubmitPreparation::Existing(existing_task_output(
            id, task,
        )));
    }
    if path.exists() {
        let task = read_task(&path)?;
        if task.execution_id != execution_id || task.request_fingerprint != fingerprint {
            return Err("conflict: 同一幂等键对应了不同的图片请求".into());
        }
        return Ok(ImageSubmitPreparation::Existing(existing_task_output(
            id, task,
        )));
    }
    let now = now_ms();
    let task = ImageTask {
        schema: 1,
        id: task_id.clone(),
        execution_id: execution_id.into(),
        request_fingerprint: fingerprint,
        status: "pending-submit".into(),
        prompt: prompt.into(),
        model: model.into(),
        actual_model: None,
        size: size.into(),
        quality: quality.map(str::to_string),
        created_at: now,
        updated_at: now,
        result: None,
        error: None,
    };
    save_task(&dir, &task)?;
    Ok(ImageSubmitPreparation::New { task_id })
}

pub fn complete_image_submit(
    id: &str,
    task_id: &str,
    raw: &str,
    actual_model: &str,
) -> Result<Value, String> {
    let dir = project_dir(id)?;
    let mut task = read_task(&task_path(&dir, task_id)?)?;
    if task.status != "pending-submit" {
        return Err("conflict: 图片任务不在可完成状态".into());
    }
    let bytes = b64_decode(raw)?;
    let extension =
        sniff_image_extension(&bytes).ok_or("invalid_input: 上游返回了未知图片签名，已拒绝落盘")?;
    let file = format!("asset-{task_id}.{extension}");
    let assets = checked_project_child(&dir, "assets")?;
    atomic_write(&assets.join(&file), &bytes)?;
    let output = json!({ "project_id": id, "task_id": task_id, "status": "completed", "asset": { "file": file, "url": format!("/__uking/v1/assets/{id}/{file}") }, "actual_model": actual_model });
    task.status = "completed".into();
    task.updated_at = now_ms();
    task.actual_model = Some(actual_model.to_string());
    task.result = Some(output.clone());
    save_task(&dir, &task)?;
    Ok(output)
}

pub fn fail_image_submit(id: &str, task_id: &str, error: &str) {
    if let Ok(dir) = project_dir(id) {
        if let Ok(path) = task_path(&dir, task_id) {
            if let Ok(mut task) = read_task(&path) {
                task.status = "failed".into();
                task.updated_at = now_ms();
                task.error = Some(error.to_string());
                let _ = save_task(&dir, &task);
            }
        }
    }
}

pub fn inspect_task(project_id: &str, task_id: &str) -> Result<Value, String> {
    if !valid_id(task_id) {
        return Err("invalid_input: task_id 格式不合法".into());
    }
    let dir = project_dir(project_id)?;
    let task = read_task(&task_path(&dir, task_id)?)?;
    Ok(json!({ "task": task }))
}

fn sniff_image_extension(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("jpg")
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("webp")
    } else {
        None
    }
}

fn b64_decode(s: &str) -> Result<Vec<u8>, String> {
    let source = s.rsplit(',').next().unwrap_or(s).trim();
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut map = [255u8; 256];
    for (i, c) in ALPHABET.iter().enumerate() {
        map[*c as usize] = i as u8;
    }
    let mut out = Vec::with_capacity(source.len() / 4 * 3);
    let mut acc = 0u32;
    let mut bits = 0u32;
    for c in source.bytes() {
        if c == b'=' || c.is_ascii_whitespace() {
            continue;
        }
        let v = map[c as usize];
        if v == 255 {
            return Err("图片 base64 含非法字符".into());
        }
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Ok(out)
}

// ───────────────────────── local loopback server ─────────────────────────

struct LocalServer {
    stop: Arc<AtomicBool>,
    threads: Vec<thread::JoinHandle<()>>,
    url: String,
    capability: String,
}
static SERVER: OnceLock<Mutex<Option<LocalServer>>> = OnceLock::new();
static COMPONENT_OPERATION: OnceLock<Mutex<()>> = OnceLock::new();
fn server_slot() -> &'static Mutex<Option<LocalServer>> {
    SERVER.get_or_init(|| Mutex::new(None))
}
fn component_operation_lock() -> &'static Mutex<()> {
    COMPONENT_OPERATION.get_or_init(|| Mutex::new(()))
}

/// The resource manifest is produced in the same staged-directory swap as the
/// OpenTu bundle.  A missing manifest is a hard error even in development:
/// serving unverified source merely because it happens to be on disk would
/// make the packaged/runtime boundary untestable.
fn verified_static_root() -> Result<PathBuf, String> {
    let root = component::active_opentu_static_root()?;
    verify_static_bundle(&root)?;
    Ok(root)
}

fn is_safe_relative_resource_path(path: &str) -> bool {
    !path.is_empty()
        && !path.contains('\\')
        && !path.contains(':')
        && Path::new(path)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path)
        .map_err(|_| "integrity_error: 无法读取 OpenTu 静态资源".to_string())?;
    let mut hasher = Sha256::new();
    let mut bytes = [0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut bytes)
            .map_err(|_| "integrity_error: 无法校验 OpenTu 静态资源".to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&bytes[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn read_resource_integrity(root: &Path) -> Result<(PathBuf, ResourceIntegrityManifest), String> {
    let root = root
        .canonicalize()
        .map_err(|_| "not_installed: OpenTu 静态包根目录不可用".to_string())?;
    let manifest_path = root.join(".uking-integrity.json");
    let manifest_path = contained_file(&root, &manifest_path)
        .ok_or("integrity_error: OpenTu 静态资源缺少完整性清单；请重新构建本地包")?;
    let manifest: ResourceIntegrityManifest = serde_json::from_slice(
        &fs::read(&manifest_path).map_err(|_| "integrity_error: 无法读取 OpenTu 完整性清单")?,
    )
    .map_err(|_| "integrity_error: OpenTu 完整性清单格式无效")?;
    if manifest.schema != 1 || manifest.files.is_empty() {
        return Err("integrity_error: OpenTu 完整性清单版本无效".into());
    }
    Ok((root, manifest))
}

fn verify_static_bundle(root: &Path) -> Result<(), String> {
    let (root, manifest) = read_resource_integrity(root)?;
    let mut seen = std::collections::BTreeSet::new();
    let mut has_index = false;
    for entry in manifest.files {
        if !is_safe_relative_resource_path(&entry.path) || !seen.insert(entry.path.clone()) {
            return Err("integrity_error: OpenTu 完整性清单包含非法资源路径".into());
        }
        if entry.path == "index.html" {
            has_index = true;
        }
        let file = contained_file(&root, &root.join(&entry.path))
            .ok_or("integrity_error: OpenTu 静态资源缺失或越界")?;
        if sha256_file(&file)? != entry.sha256 {
            return Err("integrity_error: OpenTu 静态资源校验失败；请重新构建本地包".into());
        }
    }
    has_index
        .then_some(())
        .ok_or_else(|| "integrity_error: OpenTu 完整性清单没有 index.html".into())
}

fn verify_static_file(root: &Path, file: &Path) -> bool {
    let Ok((root, manifest)) = read_resource_integrity(root) else {
        return false;
    };
    let Some(file) = contained_file(&root, file) else {
        return false;
    };
    let Ok(relative) = file.strip_prefix(&root) else {
        return false;
    };
    let relative = relative.to_string_lossy().replace('\\', "/");
    let Some(entry) = manifest.files.iter().find(|entry| entry.path == relative) else {
        return false;
    };
    sha256_file(&file)
        .map(|actual| actual == entry.sha256)
        .unwrap_or(false)
}

pub fn start_server() -> Result<Value, String> {
    if !component::CANVAS_AVAILABLE {
        return Err(component::CANVAS_COMING_SOON.into());
    }
    // An uninstall holds this same gate while it stops the listener and removes
    // the component. Without it a second surface could restart the server in
    // the narrow gap after `stop_server()` and before component deletion.
    let _component_operation = component_operation_lock()
        .lock()
        .map_err(|_| "OpenTu 组件操作锁损坏".to_string())?;
    let mut slot = server_slot().lock().map_err(|_| "本地画布服务锁损坏")?;
    if let Some(server) = slot.as_ref() {
        return Ok(json!({ "started": false, "url": server.url, "capability": server.capability }));
    }
    let root = verified_static_root()?;
    // Ask the OS for a free loopback port. A fixed port made the optional
    // canvas unusable whenever another app already listened on 17778. The
    // trusted desktop Action returns this exact origin; we never kill that app.
    let (ipv4_listener, ipv6_listener, port) = bind_loopback_listeners()?;
    ipv4_listener
        .set_nonblocking(true)
        .map_err(|e| format!("配置本地画布端口失败: {e}"))?;
    if let Some(listener) = ipv6_listener.as_ref() {
        listener
            .set_nonblocking(true)
            .map_err(|e| format!("配置 IPv6 本地创作画布端口失败: {e}"))?;
    }
    let stop = Arc::new(AtomicBool::new(false));
    let capability = random_capability()?;
    let origin = format!("http://127.0.0.1:{port}");
    let mut threads = Vec::with_capacity(usize::from(ipv6_listener.is_some()) + 1);
    if let Some(listener) = ipv6_listener {
        let thread_stop = stop.clone();
        let thread_capability = capability.clone();
        let thread_origin = origin.clone();
        let thread_root = root.clone();
        threads.push(thread::spawn(move || serve_loop(listener, thread_root, thread_capability, thread_origin, thread_stop)));
    }
    let thread_stop = stop.clone();
    let thread_capability = capability.clone();
    let thread_origin = origin.clone();
    threads.push(thread::spawn(move || serve_loop(ipv4_listener, root, thread_capability, thread_origin, thread_stop)));
    let url = format!("{origin}/");
    *slot = Some(LocalServer {
        stop,
        threads,
        url: url.clone(),
        capability: capability.clone(),
    });
    // Capability is returned only over Tauri IPC. Callers must keep it in
    // memory; it is never put in the URL, static page, log, or browser storage.
    Ok(json!({ "started": true, "url": url, "capability": capability }))
}

pub fn stop_server() -> Result<Value, String> {
    let mut slot = server_slot().lock().map_err(|_| "本地画布服务锁损坏")?;
    if let Some(server) = slot.take() {
        server.stop.store(true, Ordering::Release);
        // The serving loops own both loopback listeners. Waiting for every
        // exit proves their shared port is released before component removal.
        let mut clean_exit = true;
        for thread in server.threads {
            clean_exit &= thread.join().is_ok();
        }
        if !clean_exit {
            return Err("本地画布服务停止异常".to_string());
        }
        return Ok(json!({ "stopped": true }));
    }
    Ok(json!({ "stopped": false }))
}

fn bind_loopback_listeners() -> Result<(TcpListener, Option<TcpListener>, u16), String> {
    const PORT_ATTEMPTS: usize = 8;
    for _ in 0..PORT_ATTEMPTS {
        let ipv4_listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|e| format!("network_error: 无法启动本地创作画布服务: {e}"))?;
        let port = ipv4_listener
            .local_addr()
            .map_err(|e| format!("读取本地创作画布端口失败: {e}"))?
            .port();
        match TcpListener::bind(format!("[::1]:{port}")) {
            Ok(ipv6_listener) => return Ok((ipv4_listener, Some(ipv6_listener), port)),
            Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
                // A separate IPv6 service owns this same port. Drop the IPv4
                // listener and ask the OS for another port instead of letting
                // `localhost` resolve to somebody else's service.
                continue;
            }
            Err(error) if ipv6_loopback_unavailable(&error) => {
                if localhost_resolves_ipv6()? {
                    return Err(format!(
                        "network_error: localhost 会解析到 IPv6，但无法监听 ::1: {error}"
                    ));
                }
                // An IPv6-disabled machine resolves localhost only to IPv4;
                // serving the IPv4 listener remains the exact localhost path.
                return Ok((ipv4_listener, None, port));
            }
            Err(error) => {
                return Err(format!(
                    "network_error: 无法在同一端口监听 IPv6 本地创作画布服务: {error}"
                ));
            }
        }
    }
    Err("network_error: 无法找到同时可供 IPv4 与 IPv6 回环使用的本地创作画布端口".into())
}

fn ipv6_loopback_unavailable(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::AddrNotAvailable | std::io::ErrorKind::Unsupported
    )
}

fn localhost_resolves_ipv6() -> Result<bool, String> {
    use std::net::ToSocketAddrs;
    ("localhost", 0)
        .to_socket_addrs()
        .map(|mut addresses| addresses.any(|address| address.ip().is_ipv6()))
        .map_err(|error| format!("network_error: 无法解析 localhost: {error}"))
}

fn random_capability() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| format!("生成本地画布会话能力失败: {e}"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn serve_loop(listener: TcpListener, root: PathBuf, capability: String, origin: String, stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((stream, peer)) => {
                if peer.ip().is_loopback() {
                    let root = root.clone();
                    let capability = capability.clone();
                    let origin = origin.clone();
                    thread::spawn(move || {
                        let _ = serve_one(stream, &root, &capability, &origin);
                    });
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20))
            }
            Err(_) => break,
        }
    }
}

fn serve_one(mut stream: TcpStream, root: &Path, capability: &str, origin: &str) -> Result<(), String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    let request = read_request(&mut stream)?;
    let text = std::str::from_utf8(&request).map_err(|_| "bad request")?;
    let first = text.lines().next().ok_or("bad request")?;
    let mut words = first.split_whitespace();
    let method = words.next().unwrap_or("");
    let raw_path = words.next().unwrap_or("");
    if !raw_path.starts_with('/') || raw_path.contains('\\') {
        return reply(&mut stream, 400, "text/plain", b"bad request");
    }
    if raw_path == "/__uking/health" {
        if method != "GET" {
            return reply(&mut stream, 400, "text/plain", b"method not allowed");
        }
        return reply(
            &mut stream,
            200,
            "application/json",
            br#"{"ok":true,"service":"uking-creator"}"#,
        );
    }
    let headers = request_headers(text);
    // The desktop host reads generated assets and transfers a data URL across
    // the narrow postMessage bridge.  That is a cross-origin fetch in Tauri,
    // so answer only this exact preflight; never make the canvas origin or the
    // capability header generally CORS-readable.
    if raw_path.starts_with("/__uking/v1/assets/") && method == "OPTIONS" {
        return serve_asset_preflight(&mut stream, &headers);
    }
    if raw_path.starts_with("/__uking/v1/")
        && !constant_time_eq(
            headers
                .get("x-uking-capability")
                .map(String::as_str)
                .unwrap_or(""),
            capability,
        )
    {
        return reply(
            &mut stream,
            404,
            "application/json",
            br#"{"error":"not_found"}"#,
        );
    }
    if raw_path.starts_with("/__uking/v1/assets/") && method == "GET" {
        return serve_asset(&mut stream, raw_path, root, headers.get("origin"));
    }
    if raw_path == "/__uking/v1/action" && method == "POST" {
        return serve_action(&mut stream, text, &headers, origin);
    }
    if raw_path.starts_with("/__uking/") {
        return reply(
            &mut stream,
            404,
            "application/json",
            br#"{"error":"not_found"}"#,
        );
    }
    if method != "GET" {
        return reply(&mut stream, 400, "text/plain", b"method not allowed");
    }
    let file = match static_file(root, raw_path) {
        Some(file) => file,
        None => return reply(&mut stream, 404, "text/plain", b"not found"),
    };
    let body = fs::read(&file).map_err(|_| "not found")?;
    if !verify_static_file(root, &file) {
        return reply(
            &mut stream,
            404,
            "application/json",
            br#"{"error":"integrity_error"}"#,
        );
    }
    reply(&mut stream, 200, mime_for(&file), &body)
}

/// Resolve static assets with canonical containment. In particular, Windows
/// treats `C:/...` and `\\server\...` as rooted paths, so string checks alone
/// are not a security boundary. Reparse points are caught by canonicalizing the
/// existing target and requiring it to remain beneath the canonical bundle root.
fn static_file(root: &Path, raw_path: &str) -> Option<PathBuf> {
    let request_path = raw_path.split(['?', '#']).next().unwrap_or("");
    let relative = request_path.trim_start_matches('/');
    if relative.is_empty() {
        return contained_file(root, &root.join("index.html"));
    }
    if relative.contains(':') {
        return None;
    }
    let relative_path = Path::new(relative);
    if relative_path.is_absolute()
        || relative_path
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return None;
    }
    let candidate = root.join(relative_path);
    if candidate.is_file() {
        return contained_file(root, &candidate);
    }
    // SPA fallback is allowed only for a normal relative route; it never turns
    // an invalid absolute/traversal request into a readable local file.
    contained_file(root, &root.join("index.html"))
}
fn contained_file(root: &Path, file: &Path) -> Option<PathBuf> {
    let root = root.canonicalize().ok()?;
    let file = file.canonicalize().ok()?;
    (file.starts_with(&root) && file.is_file()).then_some(file)
}
fn read_request(stream: &mut TcpStream) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::with_capacity(4096);
    let mut chunk = [0u8; 4096];
    loop {
        let n = stream.read(&mut chunk).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..n]);
        if bytes.len() > 1024 * 1024 + 16 * 1024 {
            return Err("request too large".into());
        }
        if let Some(header_end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = std::str::from_utf8(&bytes[..header_end + 4]).map_err(|_| "bad request")?;
            let length = request_headers(head)
                .get("content-length")
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(0);
            if length > 1024 * 1024 {
                return Err("request too large".into());
            }
            if bytes.len() >= header_end + 4 + length {
                return Ok(bytes);
            }
        }
    }
    Err("incomplete request".into())
}
fn request_headers(text: &str) -> std::collections::BTreeMap<String, String> {
    text.split("\r\n")
        .skip(1)
        .take_while(|line| !line.is_empty())
        .filter_map(|line| line.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect()
}
fn constant_time_eq(left: &str, right: &str) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in left.bytes().zip(right.bytes()) {
        diff |= a ^ b;
    }
    diff == 0
}
fn trusted_host_origin(origin: &str) -> bool {
    matches!(
        origin,
        "http://tauri.localhost"
            | "https://tauri.localhost"
            | "http://localhost:1430"
            | "http://127.0.0.1:1430"
    )
}

fn requested_capability_header_only(headers: &str) -> bool {
    let requested = headers
        .split(',')
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    requested.len() == 1 && requested[0] == "x-uking-capability"
}

fn serve_asset_preflight(
    stream: &mut TcpStream,
    headers: &std::collections::BTreeMap<String, String>,
) -> Result<(), String> {
    let origin = headers.get("origin").map(String::as_str).unwrap_or("");
    let method = headers
        .get("access-control-request-method")
        .map(String::as_str)
        .unwrap_or("");
    let requested = headers
        .get("access-control-request-headers")
        .map(String::as_str)
        .unwrap_or("");
    if !trusted_host_origin(origin)
        || method != "GET"
        || !requested_capability_header_only(requested)
    {
        return reply(stream, 404, "application/json", br#"{"error":"not_found"}"#);
    }
    reply_with_headers(
        stream,
        204,
        "text/plain",
        b"",
        &[
            ("Access-Control-Allow-Origin", origin),
            ("Access-Control-Allow-Methods", "GET"),
            ("Access-Control-Allow-Headers", "X-Uking-Capability"),
            ("Access-Control-Max-Age", "300"),
            ("Vary", "Origin"),
        ],
    )
}

fn serve_asset(
    stream: &mut TcpStream,
    raw_path: &str,
    _root: &Path,
    origin: Option<&String>,
) -> Result<(), String> {
    let origin = origin.map(String::as_str).unwrap_or("");
    if !trusted_host_origin(origin) {
        return reply(stream, 404, "application/json", br#"{"error":"not_found"}"#);
    }
    let parts = raw_path
        .trim_start_matches('/')
        .split('/')
        .collect::<Vec<_>>();
    if parts.len() != 5 || !valid_id(parts[3]) {
        return reply(stream, 404, "application/json", br#"{"error":"not_found"}"#);
    }
    let file = match registered_asset_file(parts[3], parts[4]) {
        Ok(Some(file)) => file,
        Ok(None) | Err(_) => {
            return reply(stream, 404, "application/json", br#"{"error":"not_found"}"#)
        }
    };
    let body = fs::read(&file).map_err(|_| "not found")?;
    reply_with_headers(
        stream,
        200,
        mime_for(&file),
        &body,
        &[("Access-Control-Allow-Origin", origin), ("Vary", "Origin")],
    )
}

/// Resolve an asset only when the owning completed task explicitly registered
/// this exact file name.  Orphan files, hand-created names and junctions are
/// deliberately not browser-readable.
fn registered_asset_file(project_id: &str, filename: &str) -> Result<Option<PathBuf>, String> {
    let Some((asset_stem, extension)) = filename.rsplit_once('.') else {
        return Ok(None);
    };
    if !matches!(extension, "png" | "jpg" | "webp") {
        return Ok(None);
    }
    let Some(task_id) = asset_stem.strip_prefix("asset-") else {
        return Ok(None);
    };
    if !valid_id(task_id) {
        return Ok(None);
    }
    let dir = project_dir(project_id)?;
    let task = read_task(&task_path(&dir, task_id)?)?;
    let registered_name = task
        .result
        .as_ref()
        .and_then(|result| result.get("asset"))
        .and_then(|asset| asset.get("file"))
        .and_then(Value::as_str);
    if task.status != "completed" || registered_name != Some(filename) {
        return Ok(None);
    }
    let assets = checked_project_child(&dir, "assets")?;
    let file = match contained_file(&assets, &assets.join(filename)) {
        Some(file) => file,
        None => return Ok(None),
    };
    Ok(Some(file))
}
/// The loopback server deliberately does not dispatch write Actions itself.
/// A canvas is an untrusted, replaceable web surface: even with its session
/// capability it cannot turn `confirmed: true` into host approval or choose an
/// ActionParity execution id. The outer Tauri host receives the narrow
/// postMessage bridge, checks the frame origin/source/action, and creates the
/// ActionParity envelope itself. Keeping this endpoint explicit (rather than
/// accidentally falling through to static files) makes an attempted direct
/// bridge call fail closed.
fn serve_action(
    stream: &mut TcpStream,
    _text: &str,
    headers: &std::collections::BTreeMap<String, String>,
    origin: &str,
) -> Result<(), String> {
    if !headers
        .get("content-type")
        .map(|v| v.starts_with("application/json"))
        .unwrap_or(false)
    {
        return reply(
            stream,
            400,
            "application/json",
            br#"{"error":"invalid_content_type"}"#,
        );
    }
    if headers.get("origin").map(String::as_str) != Some(origin) {
        return reply(
            stream,
            400,
            "application/json",
            br#"{"error":"invalid_origin"}"#,
        );
    }
    reply(
        stream,
        403,
        "application/json",
        br#"{"error":"host_ipc_required"}"#,
    )
}
fn mime_for(path: &Path) -> &'static str {
    match path.extension().and_then(|s| s.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "webp" => "image/webp",
        _ => "application/octet-stream",
    }
}
fn reply(stream: &mut TcpStream, status: u16, mime: &str, body: &[u8]) -> Result<(), String> {
    reply_with_headers(stream, status, mime, body, &[])
}

fn reply_with_headers(
    stream: &mut TcpStream,
    status: u16,
    mime: &str,
    body: &[u8],
    extra_headers: &[(&str, &str)],
) -> Result<(), String> {
    let reason = match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        _ => "Error",
    };
    // The verified local bundle keeps executable code local. Its native
    // generation calls the configured U-King provider endpoint, while returned
    // image and media URLs may be HTTPS resources.
    const CREATOR_CANVAS_CSP: &str = "default-src 'self' data: blob:; connect-src 'self' data: blob: https://api.u-claw.org.cn; img-src 'self' https: data: blob:; media-src 'self' https: data: blob:; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; worker-src 'self' blob:";
    let mut response = format!("HTTP/1.1 {status} {reason}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nContent-Security-Policy: {CREATOR_CANVAS_CSP}\r\n", body.len());
    for (name, value) in extra_headers {
        // All values here are server constants or a validated exact Origin.
        // Keep the defensive check so a later caller cannot smuggle a header.
        if name.contains(['\r', '\n']) || value.contains(['\r', '\n']) {
            return Err("invalid response header".into());
        }
        response.push_str(name);
        response.push_str(": ");
        response.push_str(value);
        response.push_str("\r\n");
    }
    response.push_str("Connection: close\r\n\r\n");
    stream
        .write_all(response.as_bytes())
        .map_err(|e| e.to_string())?;
    stream.write_all(body).map_err(|e| e.to_string())?;
    let _ = stream.shutdown(Shutdown::Both);
    Ok(())
}

pub fn inspect_canvas() -> Value {
    let root = projects_root();
    let offer = component::inspect_opentu_offer()
        .map(serde_json::to_value)
        .and_then(|value| value.map_err(|e| e.to_string()));
    let installed = component::inspect_opentu();
    if !component::CANVAS_AVAILABLE {
        return json!({
            "ready": false,
            "release_status": "coming_soon",
            "project_root": root.display().to_string(),
            "static_bundle": false,
            "integrity": "unavailable",
            "component": installed,
            "offer": offer.unwrap_or_else(|error| json!({ "available": false, "error": error })),
            "blockers": ["创作画布待上线"],
        });
    }
    match verified_static_root() {
        Ok(_) => json!({
            "ready": true,
            "project_root": root.display().to_string(),
            "static_bundle": true,
            "integrity": "verified",
            "component": installed,
            "offer": offer.unwrap_or_else(|error| json!({ "available": false, "error": error })),
            "blockers": Vec::<String>::new(),
        }),
        Err(error) => json!({
            "ready": false,
            "project_root": root.display().to_string(),
            "static_bundle": false,
            "integrity": "unavailable",
            "component": installed,
            "offer": offer.unwrap_or_else(|catalogue_error| json!({ "available": false, "error": catalogue_error })),
            "blockers": [error],
        }),
    }
}

/// Install only the pinned catalogue entry.  It intentionally does not start
/// the iframe service: UI and CLI can inspect the completed state first.
pub fn install_canvas_component() -> Result<Value, String> {
    if !component::CANVAS_AVAILABLE {
        return Err(component::CANVAS_COMING_SOON.into());
    }
    let _operation = component_operation_lock()
        .lock()
        .map_err(|_| "OpenTu 组件操作锁损坏".to_string())?;
    let installed = component::install_catalogued_opentu()?;
    serde_json::to_value(installed).map_err(|e| e.to_string())
}

/// Stop the listener and wait for its port before deleting only the optional
/// component root.  Creator projects stay under their separate projects root.
pub fn uninstall_canvas_component() -> Result<Value, String> {
    let _operation = component_operation_lock()
        .lock()
        .map_err(|_| "OpenTu 组件操作锁损坏".to_string())?;
    let stopped = stop_server()?;
    component::uninstall_opentu(false)?;
    Ok(json!({ "removed": true, "server": stopped }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn held_canvas_cannot_start_or_install_and_keeps_projects_available() {
        crate::testsandbox::with_sandbox("creator-canvas-hold", &[], |_| {
            let project = create_project_with_id("held-project", Some("已归档项目")).unwrap();
            let state = uking_home();
            assert_eq!(start_server().unwrap_err(), component::CANVAS_COMING_SOON);
            assert_eq!(install_canvas_component().unwrap_err(), component::CANVAS_COMING_SOON);
            assert!(!state.join("components").exists(), "hold must not create a component root");

            let inspection = inspect_canvas();
            assert_eq!(inspection["ready"], false);
            assert_eq!(inspection["release_status"], "coming_soon");
            assert_eq!(inspection["offer"]["available"], false);
            assert_eq!(inspection["blockers"][0], "创作画布待上线");
            assert_eq!(inspect_project("held-project").unwrap()["id"], project["id"]);
            assert_eq!(stop_server().unwrap()["stopped"], false);
        });
    }

    #[test]
    fn projects_are_atomic_and_reject_stale_writes() {
        crate::testsandbox::with_sandbox("creator-local", &[], |_| {
            let made = create_project_with_id("demo-canvas", Some("测试")).unwrap();
            let version = made["state_version"].as_str().unwrap().to_owned();
            let saved = save_project(
                "demo-canvas",
                &json!({"children":[{"type":"image","asset":"fake.png"}]}),
                Some(&version),
            )
            .unwrap();
            assert_ne!(saved["state_version"], version);
            assert!(save_project("demo-canvas", &json!({}), Some(&version))
                .unwrap_err()
                .starts_with("conflict:"));
            let reopened = inspect_project("demo-canvas").unwrap();
            assert_eq!(reopened["canvas"]["children"][0]["asset"], "fake.png");
        });
    }

    #[test]
    fn static_route_refuses_windows_roots_and_traversal() {
        let root = std::env::temp_dir().join(format!("uking-creator-static-{}", now_ms()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("index.html"), "ok").unwrap();
        fs::write(root.join("safe.js"), "ok").unwrap();
        assert!(static_file(&root, "/safe.js").is_some());
        assert!(static_file(&root, "/../secret.txt").is_none());
        assert!(static_file(&root, "/C:/Windows/win.ini").is_none());
        assert!(static_file(&root, "/C:%2fWindows/win.ini").is_none());
        assert!(static_file(&root, "/\\\\server\\share\\secret").is_none());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn static_integrity_requires_manifest_and_detects_tampering() {
        let root = std::env::temp_dir().join(format!("uking-creator-integrity-{}", fresh_id()));
        fs::create_dir_all(&root).unwrap();
        let index = root.join("index.html");
        fs::write(&index, "safe canvas").unwrap();
        let hash = sha256_file(&index).unwrap();
        fs::write(
            root.join(".uking-integrity.json"),
            serde_json::to_vec(&json!({
                "schema": 1,
                "files": [{ "path": "index.html", "sha256": hash }],
            }))
            .unwrap(),
        )
        .unwrap();
        assert!(verify_static_bundle(&root).is_ok());
        fs::write(&index, "tampered canvas").unwrap();
        assert!(verify_static_bundle(&root)
            .unwrap_err()
            .starts_with("integrity_error:"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn asset_cors_is_limited_to_tauri_host_and_capability_header() {
        assert!(trusted_host_origin("http://tauri.localhost"));
        assert!(trusted_host_origin("http://127.0.0.1:1430"));
        assert!(!trusted_host_origin("http://127.0.0.1:17778"));
        assert!(!trusted_host_origin("https://example.invalid"));
        assert!(requested_capability_header_only("X-Uking-Capability"));
        assert!(!requested_capability_header_only(
            "X-Uking-Capability, Authorization"
        ));
        assert!(!requested_capability_header_only(""));
    }

    #[test]
    fn inline_media_canvas_is_bounded_at_32mb_without_overwriting_saved_work() {
        crate::testsandbox::with_sandbox("creator-inline-media-limit", &[], |_| {
            let made = create_project_with_id("media-canvas", None).unwrap();
            let initial_version = made["state_version"].as_str().unwrap().to_owned();
            // Three MiB is representative of one inlined generated image: it
            // exceeds the previous 2 MiB limit but stays below the new bound.
            let medium_src = format!("data:image/png;base64,{}", "a".repeat(3 * 1024 * 1024));
            let medium_canvas = json!({ "elements": [{ "type": "image", "src": medium_src }] });
            let saved = save_project("media-canvas", &medium_canvas, Some(&initial_version)).unwrap();
            let saved_version = saved["state_version"].as_str().unwrap().to_owned();
            let saved_canvas_version = saved["canvas_state_version"].clone();
            let medium_len = medium_canvas["elements"][0]["src"].as_str().unwrap().len();
            let reopened = inspect_project("media-canvas").unwrap();
            assert_eq!(reopened["canvas_state_version"], saved_canvas_version);
            assert_eq!(reopened["canvas"]["elements"][0]["src"].as_str().unwrap().len(), medium_len);

            // JSON overhead makes this definitively larger than 32 MiB. A
            // rejected replacement must leave the prior, valid canvas intact.
            let oversized_canvas = json!({
                "elements": [{
                    "type": "image",
                    "src": format!("data:image/png;base64,{}", "b".repeat(MAX_CANVAS_BYTES + 1)),
                }]
            });
            assert_eq!(
                save_project("media-canvas", &oversized_canvas, Some(&saved_version)).unwrap_err(),
                "invalid_input: 画布超过 32MB 上限"
            );
            let after_rejection = inspect_project("media-canvas").unwrap();
            assert_eq!(after_rejection["canvas_state_version"], saved_canvas_version);
            assert_eq!(
                after_rejection["canvas"]["elements"][0]["src"]
                    .as_str()
                    .unwrap()
                    .len(),
                medium_len
            );
        });
    }

    #[test]
    fn canvas_response_allows_only_the_configured_provider_connection() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            reply(&mut stream, 200, "text/plain", b"ok").unwrap();
        });
        let mut client = TcpStream::connect(address).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        server.join().unwrap();

        let connect_src = response
            .lines()
            .find_map(|header| header.strip_prefix("Content-Security-Policy: "))
            .and_then(|csp| {
                csp.split(';')
                    .map(str::trim)
                    .find(|directive| directive.starts_with("connect-src "))
            });
        assert_eq!(
            connect_src,
            Some("connect-src 'self' data: blob: https://api.u-claw.org.cn")
        );
        assert!(response.contains("img-src 'self' https: data: blob:; media-src 'self' https: data: blob:"));
    }

    #[test]
    fn loopback_server_uses_an_os_selected_port() {
        let (ipv4_listener, _, port) = bind_loopback_listeners().unwrap();
        let address = ipv4_listener.local_addr().unwrap();
        assert_eq!(address.ip().to_string(), "127.0.0.1");
        assert_eq!(address.port(), port);
        assert_ne!(port, 0);
    }

    #[test]
    fn dual_loopback_listeners_serve_health_on_the_same_port() {
        let (ipv4_listener, ipv6_listener, port) = bind_loopback_listeners().unwrap();
        let ipv4_address = ipv4_listener.local_addr().unwrap();
        ipv4_listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let root = PathBuf::from(".");
        let capability = "test-capability".to_string();
        let origin = format!("http://127.0.0.1:{port}");
        let ipv4_stop = stop.clone();
        let ipv4_thread = thread::spawn(move || {
            serve_loop(ipv4_listener, root, capability, origin, ipv4_stop)
        });

        let ipv6_thread = ipv6_listener.map(|listener| {
            let ipv6_address = listener.local_addr().unwrap();
            assert_eq!(ipv6_address.port(), port);
            listener.set_nonblocking(true).unwrap();
            let ipv6_stop = stop.clone();
            thread::spawn(move || {
                serve_loop(
                    listener,
                    PathBuf::from("."),
                    "test-capability".to_string(),
                    format!("http://127.0.0.1:{port}"),
                    ipv6_stop,
                )
            })
        });

        let health = |address: std::net::SocketAddr| {
            let mut client = TcpStream::connect_timeout(&address, Duration::from_secs(2)).unwrap();
            client
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            client
                .write_all(b"GET /__uking/health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                .unwrap();
            let mut response = String::new();
            client.read_to_string(&mut response).unwrap();
            response
        };
        assert!(health(ipv4_address).contains("200 OK"));
        if let Some(ipv6_thread) = ipv6_thread {
            let ipv6_address = std::net::SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 1], port));
            assert!(health(ipv6_address).contains("200 OK"));
            stop.store(true, Ordering::Release);
            ipv4_thread.join().unwrap();
            ipv6_thread.join().unwrap();
        } else {
            assert!(!localhost_resolves_ipv6().unwrap());
            stop.store(true, Ordering::Release);
            ipv4_thread.join().unwrap();
        }
    }

    #[test]
    fn project_list_is_newest_first_and_never_uses_directory_names_as_data() {
        crate::testsandbox::with_sandbox("creator-project-list", &[], |_| {
            create_project_with_id("project-one", Some("B")).unwrap();
            std::thread::sleep(Duration::from_millis(2));
            create_project_with_id("project-two", Some("A")).unwrap();
            // A malformed directory must not become a project picker row.
            fs::create_dir_all(projects_root().join("canvas-INVALID")).unwrap();
            let listed = list_projects().unwrap();
            assert_eq!(listed["projects"].as_array().unwrap().len(), 2);
            assert_eq!(listed["projects"][0]["id"], "project-two");
        });
    }

    #[cfg(windows)]
    #[test]
    fn project_junction_is_never_a_task_journal_search_root() {
        crate::testsandbox::with_sandbox("creator-junction-journal", &[], |_| {
            let outside =
                std::env::temp_dir().join(format!("uking-creator-outside-{}", fresh_id()));
            fs::create_dir_all(outside.join("tasks")).unwrap();
            fs::create_dir_all(projects_root()).unwrap();
            let link = projects_root().join("canvas-escape");
            let status = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(&link)
                .arg(&outside)
                .status()
                .expect("mklink command should start");
            assert!(status.success(), "test junction should be creatable");
            assert!(checked_project_dir("escape").is_err());
            let _ = fs::remove_dir(&link);
            let _ = fs::remove_dir_all(&outside);
        });
    }

    #[test]
    fn image_journal_is_written_before_provider_and_replay_never_resubmits() {
        crate::testsandbox::with_sandbox("creator-journal", &[], |_| {
            create_project_with_id("project-one", None).unwrap();
            create_project_with_id("project-two", None).unwrap();
            assert!(matches!(
                prepare_image_submit(
                    "project-one",
                    "exec-1",
                    "cat",
                    "gpt-image-2",
                    "1024x1024",
                    Some("medium")
                )
                .unwrap(),
                ImageSubmitPreparation::New { .. }
            ));
            let replay = prepare_image_submit(
                "project-one",
                "exec-1",
                "cat",
                "gpt-image-2",
                "1024x1024",
                Some("medium"),
            )
            .unwrap();
            assert!(
                matches!(replay, ImageSubmitPreparation::Existing(_)),
                "crash/restart replay must be pending-verify, not a second submit"
            );
            assert!(prepare_image_submit(
                "project-one",
                "exec-1",
                "dog",
                "gpt-image-2",
                "1024x1024",
                Some("medium")
            )
            .unwrap_err()
            .starts_with("conflict:"));
            assert!(prepare_image_submit(
                "project-two",
                "exec-1",
                "cat",
                "gpt-image-2",
                "1024x1024",
                Some("medium")
            )
            .unwrap_err()
            .starts_with("conflict:"));
        });
    }

    #[test]
    fn concurrent_delivery_claims_one_paid_submission() {
        crate::testsandbox::with_sandbox("creator-journal-concurrent", &[], |_| {
            create_project_with_id("project-one", None).unwrap();
            let submissions = std::thread::scope(|scope| {
                let handles = (0..8)
                    .map(|_| {
                        scope.spawn(|| {
                            matches!(
                                prepare_image_submit(
                                    "project-one",
                                    "exec-concurrent",
                                    "cat",
                                    "gpt-image-2",
                                    "1024x1024",
                                    Some("medium"),
                                )
                                .unwrap(),
                                ImageSubmitPreparation::New { .. }
                            )
                        })
                    })
                    .collect::<Vec<_>>();
                handles
                    .into_iter()
                    .map(|handle| handle.join().unwrap())
                    .filter(|created| *created)
                    .count()
            });
            assert_eq!(submissions, 1, "only one call may reach the provider");
        });
    }

    #[test]
    fn concurrent_saves_with_one_version_do_not_last_write_win() {
        crate::testsandbox::with_sandbox("creator-save-concurrent", &[], |_| {
            let made = create_project_with_id("project-one", None).unwrap();
            let expected = made["state_version"].as_str().unwrap().to_owned();
            let saved = std::thread::scope(|scope| {
                let handles = (0..2)
                    .map(|number| {
                        let expected = expected.clone();
                        scope.spawn(move || {
                            save_project(
                                "project-one",
                                &json!({ "children": [{ "writer": number }] }),
                                Some(&expected),
                            )
                            .is_ok()
                        })
                    })
                    .collect::<Vec<_>>();
                handles
                    .into_iter()
                    .map(|handle| handle.join().unwrap())
                    .filter(|saved| *saved)
                    .count()
            });
            assert_eq!(saved, 1, "a stale writer must receive conflict");
        });
    }

    #[test]
    fn unknown_image_signature_is_never_written_as_png() {
        crate::testsandbox::with_sandbox("creator-image-signature", &[], |_| {
            create_project_with_id("project-one", None).unwrap();
            let task_id = match prepare_image_submit(
                "project-one",
                "signature-exec",
                "cat",
                "gpt-image-2",
                "1024x1024",
                None,
            )
            .unwrap()
            {
                ImageSubmitPreparation::New { task_id } => task_id,
                other => panic!("unexpected preparation: {other:?}"),
            };
            assert!(
                complete_image_submit("project-one", &task_id, "bm90IGFuIGltYWdl", "fake")
                    .unwrap_err()
                    .contains("未知图片签名")
            );
        });
    }

    #[test]
    fn registered_asset_requires_completed_journal_and_canonical_file() {
        crate::testsandbox::with_sandbox("creator-registered-asset", &[], |_| {
            create_project_with_id("project-one", None).unwrap();
            let task_id = match prepare_image_submit(
                "project-one",
                "asset-exec",
                "cat",
                "gpt-image-2",
                "1024x1024",
                None,
            )
            .unwrap()
            {
                ImageSubmitPreparation::New { task_id } => task_id,
                other => panic!("unexpected preparation: {other:?}"),
            };
            // A valid, zlib-decodable 1×1 PNG fixture. It tests a future
            // provider completion boundary; it is never reachable via action.
            complete_image_submit(
                "project-one",
                &task_id,
                "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC",
                "test-provider",
            )
            .unwrap();
            let name = format!("asset-{task_id}.png");
            assert!(registered_asset_file("project-one", &name)
                .unwrap()
                .is_some());
            assert!(
                registered_asset_file("project-one", "image-unregistered.png")
                    .unwrap()
                    .is_none()
            );
            assert!(registered_asset_file("project-one", "asset-../secret.png")
                .unwrap()
                .is_none());
        });
    }
}
