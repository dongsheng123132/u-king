//! USB AI Genie action core. It owns only the fixed `U-King/AI-Genie` subtree.
//! In particular, inspection is deliberately a few `stat`s, never a drive walk.

use serde::Deserialize;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

const VERSION: &str = "0.3.1";
const GATEWAY: &str = "https://api.u-claw.org.cn/v1";

#[derive(Deserialize)]
struct RuntimeManifest {
    schema_version: u32,
    version: String,
    platform: String,
    sha256: String,
    asset_url: String,
    archive_bytes: u64,
}

fn manifest() -> Result<RuntimeManifest, String> {
    let m: RuntimeManifest =
        serde_json::from_str(include_str!("../resources/picoclaw-runtime.json"))
            .map_err(|e| format!("USB AI Genie runtime 清单无效: {e}"))?;
    if m.schema_version != 1
        || m.version != VERSION
        || m.platform != "windows-x64"
        || m.sha256.len() != 64
        || !m.asset_url.starts_with("https://")
        || m.archive_bytes == 0
    {
        return Err("USB AI Genie runtime 清单不符合固定 Windows x64 版本契约".into());
    }
    Ok(m)
}

/// The immutable upstream archive is cached on the host only after its pinned
/// hash is verified.  It is intentionally fetched before we create a single
/// directory on the selected U disk: a network failure must leave that disk
/// exactly as it was.  A caller may still pass `zip_path` for offline repair
/// and manufacturing; normal GUI use has no file-picker prerequisite.
fn cached_archive(m: &RuntimeManifest, progress: &crate::actions::ProgressSink) -> Result<PathBuf, String> {
    let cache_dir = std::env::temp_dir().join("u-king-usb-genie").join("runtime-cache");
    let archive = cache_dir.join(format!("picoclaw-{}-windows-x64.zip", m.version));
    if archive.is_file()
        && fs::metadata(&archive).map(|meta| meta.len() == m.archive_bytes).unwrap_or(false)
        && sha256_file(&archive).ok().as_deref() == Some(m.sha256.as_str())
    {
        return Ok(archive);
    }
    fs::create_dir_all(&cache_dir).map_err(|error| format!("创建 runtime 下载缓存失败: {error}"))?;
    let partial = cache_dir.join(format!(".picoclaw-{}-{}.part", std::process::id(), now_nanos()));
    progress("下载并校验固定 PicoClaw runtime（约 22 MB）…");
    let mut child = Command::new(crate::installer::system_tool("curl"))
        .args(["-fL", "--connect-timeout", "20", "--max-time", "600", "--retry", "2", "--retry-delay", "2", "-o"])
        .arg(&partial)
        .arg(&m.asset_url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("启动 PicoClaw 下载失败: {error}"))?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(610);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if std::time::Instant::now() < deadline => std::thread::sleep(std::time::Duration::from_millis(100)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = fs::remove_file(&partial);
                return Err("下载 PicoClaw runtime 超时，U 盘未被写入".into());
            }
            Err(error) => {
                let _ = child.kill();
                let _ = fs::remove_file(&partial);
                return Err(format!("等待 PicoClaw 下载失败: {error}"));
            }
        }
    };
    if !status.success() {
        let _ = fs::remove_file(&partial);
        return Err("下载 PicoClaw runtime 失败；请检查网络后重试，U 盘未被写入".into());
    }
    let bytes_ok = fs::metadata(&partial).map(|meta| meta.len() == m.archive_bytes).unwrap_or(false);
    let hash_ok = sha256_file(&partial).ok().as_deref() == Some(m.sha256.as_str());
    if !bytes_ok || !hash_ok {
        let _ = fs::remove_file(&partial);
        return Err("下载的 PicoClaw runtime 未通过固定大小和 SHA-256 校验，U 盘未被写入".into());
    }
    // A concurrent caller may have populated the immutable cache while this
    // one downloaded.  Replacing it with equally verified bytes is harmless.
    if archive.exists() { let _ = fs::remove_file(&archive); }
    fs::rename(&partial, &archive).map_err(|error| format!("提交 PicoClaw 下载缓存失败: {error}"))?;
    Ok(archive)
}

fn genie(root: &Path) -> PathBuf {
    root.join("U-King").join("AI-Genie")
}
fn data(root: &Path) -> PathBuf {
    genie(root).join("data")
}
fn runtime(root: &Path) -> PathBuf {
    genie(root).join("runtime")
}
fn current(root: &Path) -> PathBuf {
    runtime(root).join("current")
}
fn current_json(root: &Path) -> PathBuf {
    genie(root).join("current.json")
}
fn install_json(root: &Path) -> PathBuf {
    genie(root).join("install.json")
}
fn launcher(root: &Path) -> PathBuf {
    root.join("启动 AI 精灵.cmd")
}

#[derive(Clone, Debug)]
struct PortableTarget {
    id: String,
    root: PathBuf,
    label: String,
    filesystem: String,
    total_bytes: u64,
    free_bytes: u64,
    read_only: bool,
}

impl PortableTarget {
    fn installed(&self) -> bool {
        current_json(&self.root).is_file()
            && current(&self.root).join("picoclaw.exe").is_file()
            && data(&self.root).join("config.json").is_file()
    }

    fn target_state_version(&self) -> String {
        state_version(&self.root)
    }

    fn json(&self) -> Value {
        let version = fs::read(current_json(&self.root))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .and_then(|value| value["version"].as_str().map(str::to_owned));
        json!({
            "target_id": self.id,
            "target_root": self.root,
            "display_name": if self.label.is_empty() { self.root.display().to_string() } else { format!("{} ({})", self.label, self.root.display()) },
            "volume_label": self.label,
            "filesystem": self.filesystem,
            "total_bytes": self.total_bytes,
            "free_bytes": self.free_bytes,
            "read_only": self.read_only,
            "installed": self.installed(),
            "picoclaw_version": version,
            "program_path": genie(&self.root),
            "data_path": data(&self.root),
            "credential_path": data(&self.root).join(".security.yml"),
            "target_state_version": self.target_state_version(),
        })
    }
}

fn inventory_state_version(targets: &[PortableTarget]) -> String {
    let snapshot = targets
        .iter()
        .map(|target| format!("{}:{}", target.id, target.target_state_version()))
        .collect::<Vec<_>>()
        .join("\n");
    crate::actions::version_of(&snapshot)
}

/// This answers only whether the *U-King executable itself* was started from a
/// currently removable target.  It deliberately does not change U-King's own
/// data root: PicoClaw is the portable runtime, while U-King remains the
/// manager.  Keeping this as an explicit inspect fact prevents the UI from
/// guessing based on its installation state or a remembered drive letter.
fn launched_from_target_id(targets: &[PortableTarget], executable: &Path) -> Option<String> {
    let executable = executable.to_string_lossy().to_ascii_lowercase();
    targets.iter().find_map(|target| {
        let root = target.root.to_string_lossy().to_ascii_lowercase();
        executable.starts_with(&root).then(|| target.id.clone())
    })
}

fn target_from_inventory(input: &Value, targets: &[PortableTarget]) -> Result<PathBuf, String> {
    let text = input
        .get("target_root")
        .and_then(Value::as_str)
        .ok_or("invalid_input: target_root 必填")?;
    let target_id = input
        .get("target_id")
        .and_then(Value::as_str)
        .ok_or("invalid_input: target_id 必填")?;
    let root = PathBuf::from(text);
    if !root.is_absolute() {
        return Err("invalid_input: target_root 必须是绝对路径".into());
    }
    let matching = targets.iter().find(|candidate| {
        candidate.id == target_id
            && candidate.root.to_string_lossy().eq_ignore_ascii_case(&root.to_string_lossy())
    });
    matching
        .map(|candidate| candidate.root.clone())
        .ok_or("invalid_target: target_id 与当前可移动磁盘身份或盘符不匹配".into())
}

fn target(input: &Value) -> Result<PathBuf, String> {
    target_from_inventory(input, &portable_targets())
}

fn now_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("invalid_path: 文件没有父目录")?;
    fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {e}"))?;
    let tmp = parent.join(format!(
        ".uking-write-{}-{}",
        std::process::id(),
        now_nanos()
    ));
    fs::write(&tmp, bytes).map_err(|e| format!("写入临时文件失败: {e}"))?;
    if path.exists() {
        fs::remove_file(path).map_err(|e| format!("替换旧文件失败: {e}"))?;
    }
    fs::rename(&tmp, path).map_err(|e| format!("提交文件失败: {e}"))
}

fn state_version(root: &Path) -> String {
    let mut snapshot = String::new();
    for path in [
        current_json(root),
        data(root).join("config.json"),
        data(root).join(".security.yml"),
    ] {
        snapshot.push_str(&path.to_string_lossy());
        snapshot.push(':');
        match fs::read(&path) {
            Ok(bytes) => snapshot.push_str(&crate::installer::sha256_hex_bytes(&bytes)),
            Err(_) => snapshot.push('-'),
        }
        snapshot.push('\n');
    }
    crate::actions::version_of(&snapshot)
}

fn config_value(root: &Path) -> Value {
    // No absolute `workspace` path on purpose: the pinned value would bake the
    // manufacturing machine's drive letter into every disk (and any zip built
    // from it).  PicoClaw resolves the default workspace from PICOCLAW_HOME,
    // which the launcher always sets to this disk's data dir (verified
    // 2026-09-04: removing the field makes `status` report
    // `<PICOCLAW_HOME>\workspace` ✓; a wrong absolute path is followed
    // verbatim and marked ✗).
    json!({
        "version": 3,
        "agents": { "defaults": {
            "restrict_to_workspace": true,
            "allow_read_outside_workspace": false, "provider": "deepseek",
            "model_name": "usb-genie", "max_llm_retries": 0
        }},
        "model_list": [{
            "model_name": "usb-genie", "provider": "deepseek",
            "model": "deepseek-v4-flash", "api_base": GATEWAY, "enabled": true
        }],
        "tools": { "mcp": { "enabled": false }, "exec": { "enabled": true } }
    })
}

fn write_config(root: &Path) -> Result<(), String> {
    let path = data(root).join("config.json");
    // Preserve user-owned config fields and every model except our own pinned entry.
    // A malformed file is refused rather than silently replaced.
    let mut value = if path.exists() {
        serde_json::from_slice(&fs::read(&path).map_err(|e| e.to_string())?)
            .map_err(|e| format!("已有 config.json 无法解析，拒绝覆盖: {e}"))?
    } else {
        json!({})
    };
    merge_config(&mut value, config_value(root));
    atomic_write(
        &path,
        &serde_json::to_vec_pretty(&value).map_err(|e| e.to_string())?,
    )
}

fn merge_config(existing: &mut Value, desired: Value) {
    let Some(existing_object) = existing.as_object_mut() else {
        *existing = desired;
        return;
    };
    let desired_object = desired.as_object().expect("config template is an object");
    for (key, desired_value) in desired_object {
        if key == "model_list" {
            let mut models = existing_object
                .remove(key)
                .and_then(|v| v.as_array().cloned())
                .unwrap_or_default();
            models.retain(|model| {
                model.get("model_name").and_then(Value::as_str) != Some("usb-genie")
            });
            models.extend(desired_value.as_array().cloned().unwrap_or_default());
            existing_object.insert(key.clone(), Value::Array(models));
        } else if let Some(current) = existing_object.get_mut(key) {
            if current.is_object() && desired_value.is_object() {
                merge_config(current, desired_value.clone());
            } else {
                *current = desired_value.clone();
            }
        } else {
            existing_object.insert(key.clone(), desired_value.clone());
        }
    }
    // Migration: config written before 2026-09-04 baked an absolute workspace
    // path (the manufacturing machine's drive letter) into agents.defaults.
    // PicoClaw follows that path verbatim and never falls back to
    // PICOCLAW_HOME, so a stale field breaks every disk that changes drives.
    // Drop it after merging — the launcher's PICOCLAW_HOME always provides the
    // right default (verified live 2026-09-04).
    if let Some(defaults) = existing_object
        .get_mut("agents")
        .and_then(|agents| agents.as_object_mut())
        .and_then(|agents| agents.get_mut("defaults"))
        .and_then(|defaults| defaults.as_object_mut())
    {
        defaults.remove("workspace");
    }
}

fn sha256_file(path: &Path) -> Result<String, String> {
    Ok(crate::installer::sha256_hex_bytes(
        &fs::read(path).map_err(|e| format!("读取文件失败: {e}"))?,
    ))
}

fn artifact_hashes(root: &Path) -> Result<Value, String> {
    Ok(json!({
        "runtime/current/picoclaw.exe": sha256_file(&current(root).join("picoclaw.exe"))?,
        "runtime/current/LICENSE": sha256_file(&current(root).join("LICENSE"))?,
        "runtime/current/README.md": sha256_file(&current(root).join("README.md"))?,
        "launcher": sha256_file(&launcher(root))?,
    }))
}

fn artifacts_match(root: &Path, expected: &Value) -> bool {
    artifact_hashes(root).ok().as_ref() == Some(expected)
}

/// P1 duplicate-launch guard: the target is "busy" iff a running picoclaw.exe
/// was actually started from THIS target's runtime directory.  The first cut
/// recorded the intermediate cmd.exe PID in running.json and trusted it — but
/// that cmd exits as soon as the agent detaches, so dedup never fired and
/// repeated launches stacked extra agents (found on real disk 2026-09-04).
/// Discovery by image name alone is forbidden by house rules; we pair every
/// PID with its own executable path and match the path against this target
/// only.  Known P1 residual ambiguity: two USB disks that swapped drive
/// letters between sessions look identical until re-plugged in order.
#[cfg(windows)]
fn picoclaw_agent_busy(root: &Path) -> Option<u32> {
    use std::os::windows::process::CommandExt;
    let needle = current(root).join("picoclaw.exe").to_string_lossy().to_lowercase();
    let listing = Command::new("tasklist.exe")
        .args(["/FI", "IMAGENAME eq picoclaw.exe", "/NH", "/FO", "CSV"])
        .creation_flags(0x0800_0000)
        .output()
        .ok()?;
    let rows = String::from_utf8_lossy(&listing.stdout)
        .lines()
        .filter_map(|line| line.split("\",\"").nth(1)?.trim_matches('"').parse::<u32>().ok().map(|_| ()))
        .count();
    if rows == 0 {
        return None; // fast path: no picoclaw anywhere, no path query needed
    }
    let probe = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", "Get-Process -Name picoclaw -ErrorAction SilentlyContinue | ForEach-Object { '{0}|{1}' -f $_.Id, $_.Path }"])
        .creation_flags(0x0800_0000)
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&probe.stdout);
    for line in text.lines() {
        let (pid, path) = line.trim().split_once('|')?;
        if path.to_lowercase().ends_with(&needle) {
            if let Ok(pid) = pid.parse::<u32>() {
                return Some(pid);
            }
        }
    }
    None
}
#[cfg(not(windows))]
fn picoclaw_agent_busy(_: &Path) -> Option<u32> { None }

fn runtime_files_are_valid(dir: &Path) -> Result<(), String> {
    for name in ["picoclaw.exe", "LICENSE", "README.md"] {
        if !dir.join(name).is_file() {
            return Err(format!("runtime 缺少受允许文件 {name}"));
        }
    }
    let output = Command::new(dir.join("picoclaw.exe"))
        .arg("version")
        .output()
        .map_err(|error| format!("验证 staging PicoClaw 失败: {error}"))?;
    if !output.status.success() || !String::from_utf8_lossy(&output.stdout).contains(VERSION) {
        return Err("staging PicoClaw 版本不符合固定版本契约".into());
    }
    Ok(())
}

fn staging_runtime(root: &Path) -> PathBuf {
    runtime(root).join(format!(".uking-stage-{}-{}", std::process::id(), now_nanos()))
}

/// Commit only a fully extracted and executable immutable version directory.
/// P1 pins exactly one runtime, so an existing valid version is retained rather
/// than overwritten. `current` is populated once and never replaced in place.
fn stage_and_commit_runtime(root: &Path, zip: &Path, progress: &crate::actions::ProgressSink) -> Result<(), String> {
    fs::create_dir_all(runtime(root)).map_err(|error| format!("创建 runtime 目录失败: {error}"))?;
    let staged = staging_runtime(root);
    fs::create_dir_all(&staged).map_err(|error| format!("创建 staging 目录失败: {error}"))?;
    let outcome = (|| {
        progress("解压固定 PicoClaw runtime 到同盘 staging…");
        extract_runtime(zip, &staged)?;
        runtime_files_are_valid(&staged)?;
        let versioned = runtime(root).join(format!("picoclaw-{VERSION}"));
        if versioned.exists() {
            runtime_files_are_valid(&versioned)?;
        } else {
            fs::rename(&staged, &versioned).map_err(|error| format!("提交 PicoClaw runtime 失败: {error}"))?;
        }
        let stable = current(root);
        if stable.exists() {
            runtime_files_are_valid(&stable)?;
        } else {
            fs::create_dir(&stable).map_err(|error| format!("创建 stable runtime 失败: {error}"))?;
            for name in ["picoclaw.exe", "LICENSE", "README.md"] {
                fs::copy(versioned.join(name), stable.join(name)).map_err(|error| format!("准备 stable runtime 失败: {error}"))?;
            }
            runtime_files_are_valid(&stable)?;
        }
        Ok(())
    })();
    // Only delete a directory created by this invocation. A committed staging
    // directory was renamed and therefore no longer exists at this path.
    if staged.exists() { let _ = fs::remove_dir_all(&staged); }
    outcome
}

#[cfg(windows)]
fn extract_runtime(zip: &Path, destination: &Path) -> Result<(), String> {
    // ZipArchive is in .NET and extracts exactly the three allowed files. Paths
    // arrive through private child environment variables: PowerShell `-Command`
    // otherwise parses appended argv as part of the command text.
    let script = "Add-Type -AssemblyName System.IO.Compression.FileSystem;$z=[IO.Compression.ZipFile]::OpenRead($env:UKING_USB_GENIE_ZIP);try{$want=@{'picoclaw.exe'='picoclaw.exe';'LICENSE'='LICENSE';'README.md'='README.md'};foreach($e in $z.Entries){$n=[IO.Path]::GetFileName($e.FullName);if($want.ContainsKey($n) -and -not [string]::IsNullOrEmpty($e.Name)){$out=Join-Path $env:UKING_USB_GENIE_DEST $want[$n];[IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($out))|Out-Null;$s=$e.Open();try{$d=[IO.File]::Create($out);try{$s.CopyTo($d)}finally{$d.Dispose()}}finally{$s.Dispose()}}}}finally{$z.Dispose()}";
    let out = Command::new("powershell.exe")
        .env("UKING_USB_GENIE_ZIP", zip)
        .env("UKING_USB_GENIE_DEST", destination)
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .map_err(|e| format!("解压 PicoClaw 失败: {e}"))?;
    if !out.status.success() {
        return Err("解压 PicoClaw runtime 失败".into());
    }
    for name in ["picoclaw.exe", "LICENSE", "README.md"] {
        if !destination.join(name).is_file() {
            return Err(format!("runtime 压缩包缺少 {name}"));
        }
    }
    Ok(())
}

#[cfg(not(windows))]
fn extract_runtime(_: &Path, _: &Path) -> Result<(), String> {
    Err("USB AI Genie 一期仅支持 Windows x64".into())
}

fn write_security(root: &Path, key: &str) -> Result<(), String> {
    // Keep the key in the shortest possible scope. It is never put in a command,
    // progress message, error, or result object.
    //
    // Shape contract: PicoClaw only reads api_keys from the per-model entry
    // under `model_list:` — byte-for-byte the shape `picoclaw model add`
    // writes (verified 2026-09-04 on a real tool disk: a top-level
    // `usb-genie:0:` mapping is silently ignored and every gateway call then
    // fails with 401 "Invalid token").  The document is therefore written as
    // the literal native template (key is a hex token, no injection surface),
    // not round-tripped through serde_yaml — serde_yaml prints empty maps
    // inline (`dingtalk: {}`) where picoclaw writes nested
    // (`dingtalk:\n  settings: {}`), and we refuse "close enough" on the
    // credential file.
    let mut doc = String::from("channel_list:\n");
    for channel in ["dingtalk", "discord", "feishu", "irc", "line", "maixcam", "matrix", "onebot", "pico", "qq", "slack", "telegram", "wecom", "weixin", "whatsapp"] {
        doc.push_str(&format!("  {channel}:\n    settings: {{}}\n"));
    }
    doc.push_str("model_list:\n  usb-genie:0:\n    api_keys:\n      - ");
    doc.push_str(key);
    doc.push_str("\nweb:\n");
    for engine in ["brave", "tavily", "kagi", "gemini", "perplexity", "glm_search", "baidu_search"] {
        doc.push_str(&format!("  {engine}: {{}}\n"));
    }
    doc.push_str("skills:\n  registries: {}\n");
    atomic_write(
        &data(root).join(".security.yml"),
        doc.as_bytes(),
    )
}

/// Resolve every credential decision before the first filesystem write.  `none`
/// means “do not add or replace a credential”; it never means “quietly delete
/// whatever was already on this tool disk”.
fn credential_plan(root: &Path, credential_ref: &str, official_device_key: Option<String>) -> Result<(Option<String>, &'static str), String> {
    match credential_ref {
        "none" => Ok((None, if data(root).join(".security.yml").is_file() { "preserved_existing" } else { "none" })),
        "official_device" => Ok((Some(official_device_key.ok_or("credential_unavailable: 当前设备钱包不可用")?), "official_device")),
        _ => Err("invalid_input: credential_ref 目前只能是 none 或 official_device".into()),
    }
}

/// Space preflight for deploy (gates:21).  64 MiB ≈ 3x the pinned runtime
/// archive; only refuses targets that cannot possibly hold the result.
const MIN_DEPLOY_FREE_BYTES: u64 = 64 * 1024 * 1024;
fn ensure_space(free_bytes: u64) -> Result<(), String> {
    if free_bytes < MIN_DEPLOY_FREE_BYTES {
        Err(format!(
            "insufficient_space: 目标盘剩余 {:.0} MB，制作 AI 精灵至少需要 64 MB（runtime 约 22 MB + 工作区余量），未写入任何文件",
            free_bytes as f64 / (1024.0 * 1024.0)
        ))
    } else {
        Ok(())
    }
}

fn owns_target(root: &Path) -> bool {
    fs::read(install_json(root))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|value| value["product"].as_str().map(str::to_owned))
        .as_deref() == Some("uking-usb-ai-genie")
}

/// One migration bridge for P1 disks made before install.json existed.  It is
/// intentionally narrow: all three U-King-controlled artifacts must exist;
/// an arbitrary same-name folder is never adopted.
fn known_legacy_target(root: &Path) -> bool {
    current_json(root).is_file()
        && current(root).join("picoclaw.exe").is_file()
        && data(root).join("config.json").is_file()
        && launcher(root).is_file()
}

fn preflight_ownership(root: &Path) -> Result<(), String> {
    let genie_exists = genie(root).exists();
    let launcher_exists = launcher(root).exists();
    if (genie_exists || launcher_exists) && !owns_target(root) && !known_legacy_target(root) {
        return Err("target_conflict: 目标盘已有同名 AI-Genie 目录或启动器，但不是 U-King 已管理的工具盘；为保护原文件已拒绝覆盖".into());
    }
    Ok(())
}

fn write_install_marker(root: &Path) -> Result<(), String> {
    atomic_write(
        &install_json(root),
        &serde_json::to_vec_pretty(&json!({
            "schema_version": 1,
            "product": "uking-usb-ai-genie",
            "runtime": "picoclaw",
            "created_by": "U-King",
        })).map_err(|error| error.to_string())?,
    )
}

fn deploy(
    root: &Path,
    credential_ref: &str,
    official_device_key: Option<String>,
    zip: &Path,
    progress: &crate::actions::ProgressSink,
) -> Result<Value, String> {
    let m = manifest()?;
    if !zip.is_file() {
        return Err("invalid_input: zip_path 必须指向本地 PicoClaw v0.3.1 压缩包".into());
    }
    progress("校验固定 PicoClaw runtime 压缩包…");
    if sha256_file(zip)? != m.sha256 {
        return Err("runtime 压缩包 SHA-256 与固定清单不匹配".into());
    }
    // All inputs that can fail without requiring disk I/O are resolved before
    // we create any target directory. In particular, an unsupported provider
    // must not leave a half-created runtime behind.
    let (credential_to_write, credential_mode) = credential_plan(root, credential_ref, official_device_key)?;
    preflight_ownership(root)?;
    let fresh_target = !genie(root).exists() && !launcher(root).exists();
    let outcome = (|| {
        // Runtime is committed before any data directory exists.  Thus a
        // broken archive / extraction can never be mistaken for a half-ready
        // AI data tree.
        stage_and_commit_runtime(root, zip, progress)?;
        for dir in [data(root).join("workspace"), data(root).join("logs"), data(root).join("tmp")] {
            fs::create_dir_all(dir).map_err(|e| format!("创建 AI Genie 目录失败: {e}"))?;
        }
        progress("生成 AI Genie 配置与启动器…");
        write_config(root)?;
        if let Some(key) = credential_to_write {
            write_security(root, &key)?;
        }
        atomic_write(
            &launcher(root),
            include_bytes!("../resources/usb-genie/launch-agent.cmd"),
        )?;
        atomic_write(&current_json(root), serde_json::to_vec_pretty(&json!({"schema_version":1,"version":m.version,"archive_sha256":m.sha256,"runtime_dir":format!("picoclaw-{VERSION}"),"credential_mode":credential_mode,"artifact_hashes":artifact_hashes(root)?})).map_err(|e| e.to_string())?.as_slice())?;
        write_install_marker(root)?;
        let verification = verify(root)?;
        if !verification["ok"].as_bool().unwrap_or(false) {
            return Err("制作后验证失败".into());
        }
        Ok(json!({"changed":true,"target_root":root,"picoclaw_version":VERSION,"sha256_ok":true,"credential_mode":credential_mode,"target_state_version":state_version(root)}))
    })();
    if outcome.is_err() && fresh_target {
        // Preflight proved both paths were absent.  Remove only the subtree we
        // created, never an existing or unowned target.  A disconnected or
        // locked drive simply keeps the truthful error; it never gets a false
        // completed marker.
        let _ = fs::remove_dir_all(genie(root));
        if fs::read(launcher(root)).ok().as_deref() == Some(include_bytes!("../resources/usb-genie/launch-agent.cmd")) {
            let _ = fs::remove_file(launcher(root));
        }
    }
    outcome
}

fn verify(root: &Path) -> Result<Value, String> {
    let m = manifest()?;
    let exe = current(root).join("picoclaw.exe");
    let versioned_exe = runtime(root)
        .join(format!("picoclaw-{VERSION}"))
        .join("picoclaw.exe");
    let current_meta = fs::read(current_json(root))
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok());
    let config = fs::read(data(root).join("config.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok());
    let config_ok = config
        .as_ref()
        .and_then(|v| v["model_list"].as_array())
        .map(|models| {
            models
                .iter()
                .any(|x| x["provider"] == "deepseek" && x["api_base"] == GATEWAY)
        })
        .unwrap_or(false);
    let runtime_meta_ok = current_meta
        .as_ref()
        .map(|v| v["version"] == VERSION && v["archive_sha256"] == m.sha256)
        .unwrap_or(false);
    let artifacts_ok = current_meta
        .as_ref()
        .and_then(|value| value.get("artifact_hashes"))
        .map(|expected| artifacts_match(root, expected))
        .unwrap_or(false);
    let credential_absent_when_none = current_meta
        .as_ref()
        .map(|v| v["credential_mode"] != "none" || !data(root).join(".security.yml").exists())
        .unwrap_or(false);
    let files_ok = exe.is_file()
        && versioned_exe.is_file()
        && current(root).join("LICENSE").is_file()
        && current(root).join("README.md").is_file();
    let launcher_ok = launcher(root).is_file()
        && fs::read(launcher(root))
            .map(|b| b.is_ascii())
            .unwrap_or(false);
    let version_ok = if exe.is_file() {
        Command::new(&exe)
            .arg("version")
            .output()
            .ok()
            .map(|o| o.status.success() && String::from_utf8_lossy(&o.stdout).contains(VERSION))
            .unwrap_or(false)
    } else {
        false
    };
    let checks = json!({
        "runtime_files": {"ok":files_ok,"detail":"current and pinned versioned runtime contain the three allowlisted files"},
        "runtime_manifest": {"ok":runtime_meta_ok,"detail":"current.json matches the pinned archive hash"},
        "artifact_hashes": {"ok":artifacts_ok,"detail":"runtime and launcher bytes match hashes recorded at successful install"},
        "picoclaw_version": {"ok":version_ok,"detail":"picoclaw.exe version reports 0.3.1"},
        "launcher": {"ok":launcher_ok,"detail":"root launcher exists and is ASCII-only"},
        "config": {"ok":config_ok,"detail":"model provider is deepseek and uses the China gateway"},
        "credential_template": {"ok":credential_absent_when_none,"detail":"credential-free templates contain no .security.yml"}
    });
    let mut blockers = Vec::new();
    for (name, check) in checks.as_object().unwrap() {
        if !check["ok"].as_bool().unwrap_or(false) {
            blockers.push(format!("验证失败: {name}"));
        }
    }
    Ok(
        json!({"ok":blockers.is_empty(),"checks":checks,"blockers":blockers,"target_state_version":state_version(root)}),
    )
}

/// One raw drive slot as reported by Windows, before any USB-Genie-specific
/// filtering (removable-only, ownership, etc). Shared with `installer.rs`'s
/// tool path discovery so the two features never maintain two separate drive
/// scans (CLAUDE.md 第 8/13 条: 同一事实只查一次，公共能力复用不复制).
#[derive(Clone, Debug)]
pub(crate) struct DriveSlot {
    pub root: PathBuf,
    /// Raw `GetDriveTypeW` value: 2 = removable, 3 = fixed, others = remote/CD/ramdisk/unknown.
    pub drive_type: u32,
}

pub(crate) const DRIVE_TYPE_REMOVABLE: u32 = 2;
pub(crate) const DRIVE_TYPE_FIXED: u32 = 3;

/// Enumerate every assigned drive letter and its raw type. Deliberately just
/// `GetLogicalDrives` + `GetDriveTypeW` per letter — both are cheap local
/// bitmask/registry-ish lookups, not filesystem I/O, so this alone cannot
/// block on a disconnected mapped drive. Callers still must not call
/// `Path::exists()` etc. on the returned roots without their own timeout.
#[cfg(windows)]
pub(crate) fn enumerate_drive_slots() -> Vec<DriveSlot> {
    extern "system" {
        fn GetLogicalDrives() -> u32;
        fn GetDriveTypeW(root: *const u16) -> u32;
    }
    let mask = unsafe { GetLogicalDrives() };
    (0..26)
        .filter_map(|index| {
            if mask & (1 << index) == 0 {
                return None;
            }
            let root = format!("{}:\\", (b'A' + index as u8) as char);
            let wide: Vec<u16> = root.encode_utf16().chain(Some(0)).collect();
            let drive_type = unsafe { GetDriveTypeW(wide.as_ptr()) };
            Some(DriveSlot { root: PathBuf::from(root), drive_type })
        })
        .collect()
}
#[cfg(not(windows))]
pub(crate) fn enumerate_drive_slots() -> Vec<DriveSlot> {
    Vec::new()
}

#[cfg(windows)]
fn portable_targets() -> Vec<PortableTarget> {
    extern "system" {
        fn GetVolumeInformationW(
            root: *const u16,
            volume_name: *mut u16,
            volume_name_len: u32,
            serial: *mut u32,
            maximum_component_len: *mut u32,
            flags: *mut u32,
            filesystem_name: *mut u16,
            filesystem_name_len: u32,
        ) -> i32;
        fn GetDiskFreeSpaceExW(
            root: *const u16,
            available: *mut u64,
            total: *mut u64,
            total_free: *mut u64,
        ) -> i32;
        fn GetVolumeNameForVolumeMountPointW(
            root: *const u16,
            volume_name: *mut u16,
            volume_name_len: u32,
        ) -> i32;
    }
    enumerate_drive_slots()
        .into_iter()
        .filter_map(|slot| {
            if slot.drive_type != DRIVE_TYPE_REMOVABLE {
                return None;
            }
            let root = slot.root.to_string_lossy().into_owned();
            let wide: Vec<u16> = root.encode_utf16().chain(Some(0)).collect();
            let mut label = [0u16; 261];
            let mut filesystem = [0u16; 261];
            let mut serial = 0u32;
            let mut ignored = 0u32;
            let volume_ok = unsafe {
                GetVolumeInformationW(
                    wide.as_ptr(), label.as_mut_ptr(), label.len() as u32, &mut serial,
                    &mut ignored, &mut ignored, filesystem.as_mut_ptr(), filesystem.len() as u32,
                )
            } != 0;
            if !volume_ok { return None; }
            let mut volume_guid = [0u16; 261];
            let guid_ok = unsafe {
                GetVolumeNameForVolumeMountPointW(wide.as_ptr(), volume_guid.as_mut_ptr(), volume_guid.len() as u32)
            } != 0;
            let mut available = 0u64;
            let mut total = 0u64;
            let mut total_free = 0u64;
            let space_ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut available, &mut total, &mut total_free) } != 0;
            let to_string = |buf: &[u16]| String::from_utf16_lossy(&buf[..buf.iter().position(|&c| c == 0).unwrap_or(buf.len())]);
            let guid = if guid_ok { to_string(&volume_guid) } else { format!("serial-{serial:08X}") };
            Some(PortableTarget {
                // Volume GUID survives a drive-letter change; serial is retained to make accidental GUID/API failures explicit.
                id: format!("windows:{guid}:{serial:08X}"),
                root: PathBuf::from(root),
                label: to_string(&label),
                filesystem: to_string(&filesystem),
                total_bytes: if space_ok { total } else { 0 },
                free_bytes: if space_ok { available } else { 0 },
                read_only: false,
            })
        })
        .collect()
}
#[cfg(not(windows))]
fn portable_targets() -> Vec<PortableTarget> {
    Vec::new()
}

pub fn inspect() -> Result<Value, String> {
    let target_records = portable_targets();
    let targets = target_records.iter().map(PortableTarget::json).collect::<Vec<_>>();
    let ready = !targets.is_empty();
    let inventory = inventory_state_version(&target_records);
    let launched_from_target_id = std::env::current_exe()
        .ok()
        .and_then(|exe| launched_from_target_id(&target_records, &exe));
    Ok(
        json!({"schema_version":2,"ready":ready,"blockers":if ready {Vec::<String>::new()} else {vec!["未检测到可移动磁盘".to_string()]},"targets":targets,"launched_from_target_id":launched_from_target_id,"inventory_state_version":inventory,"state_version":inventory}),
    )
}

fn current_inventory_state() -> String {
    inventory_state_version(&portable_targets())
}

fn filesystem_supported(filesystem: &str) -> bool {
    !filesystem.eq_ignore_ascii_case("FAT32")
}

/// The Action framework currently guards writes with a whole-inventory snapshot,
/// whereas runtime health is target-local. Keep both names explicit: consumers
/// must never mistake a target's state for the inventory concurrency token.
fn attach_inventory_state(mut result: Value) -> Value {
    let inventory = current_inventory_state();
    if let Some(object) = result.as_object_mut() {
        object.insert("inventory_state_version".into(), Value::String(inventory.clone()));
        object.insert("state_version".into(), Value::String(inventory));
    }
    result
}

/// `Action::state_fn` has no access to an action's input, so it must use the
/// aggregate snapshot that `inspect` returns. Any changed removable target makes a
/// stale write fail before its handler touches disk.
pub fn action_state_version() -> String {
    current_inventory_state()
}

pub fn action_inspect(
    _: &str,
    _: Value,
    _: &crate::actions::ProgressSink,
) -> Result<Value, String> {
    inspect()
}
pub fn action_deploy_with_device_key(
    _: &str,
    input: Value,
    progress: &crate::actions::ProgressSink,
    official_device_key: Option<String>,
) -> Result<Value, String> {
    let targets = portable_targets();
    let root = target_from_inventory(&input, &targets)?;
    let target = targets.iter().find(|candidate| candidate.root == root)
        .ok_or("invalid_target: 目标盘在制作前已消失")?;
    if !filesystem_supported(&target.filesystem) {
        return Err("unsupported_filesystem: FAT32 未通过便携 AI 的原子提交验收；请使用 NTFS 或 exFAT U 盘，未写入任何文件".into());
    }
    // Space preflight (gates:21 "首次写入前完成输入、身份、空间、文件系统…全部检查"):
    // the payload is the runtime extract plus launcher and config; 64 MiB is a
    // generous constant — 3x the ~22 MB pinned archive — and only refuses
    // targets that cannot possibly hold the result.  No bytes are written by
    // the checks above, so rejection here is still "未写入任何文件".
    ensure_space(target.free_bytes)?;
    let credential = input
        .get("credential_ref")
        .and_then(Value::as_str)
        .ok_or("invalid_input: credential_ref 必填")?;
    let zip = match input.get("zip_path").and_then(Value::as_str).filter(|path| !path.trim().is_empty()) {
        Some(path) => PathBuf::from(path),
        None => cached_archive(&manifest()?, progress)?,
    };
    Ok(attach_inventory_state(deploy(&root, credential, official_device_key, &zip, progress)?))
}
pub fn action_verify(
    _: &str,
    input: Value,
    _: &crate::actions::ProgressSink,
) -> Result<Value, String> {
    Ok(attach_inventory_state(verify(&target(&input)?)?))
}
pub fn action_launch(
    _: &str,
    input: Value,
    _: &crate::actions::ProgressSink,
) -> Result<Value, String> {
    let root = target(&input)?;
    let cmd = launcher(&root);
    if !cmd.is_file() {
        return Err("not_ready: AI Genie 启动器不存在，请先制作".into());
    }
    // A root launcher existing is not sufficient evidence that it still points
    // at the pinned runtime.  Verify the exact selected target immediately
    // before opening an interactive console; never fall through to a same-name
    // executable elsewhere on the host.
    let verification = verify(&root)?;
    if !verification["ok"].as_bool().unwrap_or(false) {
        return Err("not_ready: AI Genie 验证未通过，请先检查或修复此 U 盘".into());
    }
    if let Some(pid) = picoclaw_agent_busy(&root) {
        return Ok(attach_inventory_state(json!({"changed":false,"launched":false,"already_running":true,"pid":pid,"target_state_version":state_version(&root)})));
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // `cmd /c start` allocates a REAL console for the interactive agent.
        // Spawning the launcher directly inherited this CLI's piped stdio, so
        // picoclaw saw EOF and exited instantly ("Goodbye" within 2s, zero log
        // lines — reproduced 2026-09-04).  With a fresh console the same
        // launcher stays interactive.  The started cmd is transient: P1 does
        // not track its PID (dedup above keys on the picoclaw process itself).
        //
        // stdio must be fully detached: a child that inherits the caller's
        // stdout pipe keeps the pipe open for its whole lifetime, and a CLI
        // `action run` caller then blocks on EOF forever (observed 2026-09-04:
        // the action had already succeeded while the shell hung on `tail`).
        use std::process::{Command as StdCommand, Stdio};
        StdCommand::new("cmd.exe")
            .args(["/d", "/c", "start", "U-King USB AI Genie", "/D", &root.to_string_lossy(), "/MIN", &cmd.to_string_lossy()])
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW for the tiny dispatcher
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("启动 AI Genie 失败: {e}"))?;
        return Ok(attach_inventory_state(json!({"changed":true,"launched":true,"already_running":false,"target_state_version":state_version(&root)})));
    }
    #[cfg(not(windows))]
    {
        return Err("USB AI Genie 一期仅支持 Windows x64".into());
    }
    #[allow(unreachable_code)]
    Ok(attach_inventory_state(json!({"changed":true,"launched":true,"already_running":false,"target_state_version":state_version(&root)})))
}
pub fn action_credential_remove(
    _: &str,
    input: Value,
    _: &crate::actions::ProgressSink,
) -> Result<Value, String> {
    let root = target(&input)?;
    let path = data(&root).join(".security.yml");
    let fingerprint = fs::read(&path)
        .ok()
        .map(|b| crate::installer::sha256_hex_bytes(&b)[..12].to_string());
    let removed = if path.exists() {
        fs::remove_file(&path).map_err(|e| format!("删除凭据失败: {e}"))?;
        true
    } else {
        false
    };
    Ok(
        attach_inventory_state(json!({"changed":removed,"removed":removed,"previous_fingerprint":fingerprint,"target_state_version":state_version(&root)})),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn root() -> PathBuf {
        std::env::temp_dir().join(format!("uking-usb-genie-{}", now_nanos()))
    }
    #[test]
    fn manifest_is_pinned() {
        let m = manifest().unwrap();
        assert_eq!(m.version, VERSION);
        assert_eq!(m.sha256.len(), 64);
        assert!(m.asset_url.starts_with("https://"));
        assert!(m.archive_bytes > 20_000_000);
    }
    #[test]
    fn fat32_is_refused_before_any_target_write() {
        assert!(!filesystem_supported("FAT32"));
        assert!(!filesystem_supported("fat32"));
        assert!(filesystem_supported("exFAT"));
        assert!(filesystem_supported("NTFS"));
    }
    #[test]
    fn config_has_no_absolute_workspace_so_zips_stay_portable() {
        // 契约：config 不得写绝对 workspace 路径——绝对路径会把制作机的盘符烙进
        // 每一块盘和每一个发布 zip（真盘实测 2026-09-04：picoclaw 对错误绝对路径
        // 照单全收并标 ✗，不回退 PICOCLAW_HOME；删字段则回退 <PICOCLAW_HOME>\workspace ✓）。
        let p = root();
        let v = config_value(&p);
        assert!(
            v["agents"]["defaults"].get("workspace").is_none(),
            "workspace must resolve from PICOCLAW_HOME, not a baked absolute path"
        );
        assert_eq!(v["model_list"][0]["provider"], "deepseek");
        assert_eq!(v["model_list"][0]["api_base"], GATEWAY);
    }
    #[test]
    fn config_merge_keeps_unknown_fields_and_other_models() {
        let p = root();
        let mut old = json!({"custom":{"keep":true},"model_list":[{"model_name":"other","provider":"other"}],"agents":{"defaults":{"custom":true}}});
        merge_config(&mut old, config_value(&p));
        assert_eq!(old["custom"]["keep"], true);
        assert_eq!(old["agents"]["defaults"]["custom"], true);
        assert_eq!(old["model_list"][0]["model_name"], "other");
        assert_eq!(old["model_list"][1]["provider"], "deepseek");
    }
    #[test]
    fn launcher_is_ascii_and_uses_stable_current_path() {
        let b = include_bytes!("../resources/usb-genie/launch-agent.cmd");
        assert!(b.is_ascii());
        let s = std::str::from_utf8(b).unwrap();
        for wanted in [
            "PICOCLAW_HOME",
            "PICOCLAW_CONFIG",
            "PICOCLAW_BINARY",
            "PICOCLAW_BUILTIN_SKILLS",
            "PICOCLAW_LOG_FILE",
            "TEMP=",
            "TMP=",
            "runtime\\current\\picoclaw.exe",
        ] {
            assert!(s.contains(wanted));
        }
    }
    #[test]
    fn state_changes_when_credential_is_removed() {
        let p = root();
        fs::create_dir_all(data(&p)).unwrap();
        fs::write(
            data(&p).join(".security.yml"),
            "usb-genie:0:\n  api_keys:\n    - sk-abc123\n",
        )
        .unwrap();
        let before = state_version(&p);
        // This verifies the file-state contract without pretending a temporary
        // test directory is a physical removable drive.  The public Action is
        // separately required to reject such arbitrary paths.
        let path = data(&p).join(".security.yml");
        fs::remove_file(path).unwrap();
        assert_ne!(before, state_version(&p));
        let _ = fs::remove_dir_all(p);
    }
    #[test]
    fn write_actions_reject_non_removable_roots_before_touching_disk() {
        let p = root();
        let error = crate::actions::run(
            crate::actions::USB_GENIE_DEPLOY,
            json!({
                "confirm": true,
                "target_id": "test-volume",
                "target_root": p,
                "credential_ref": "none",
                "zip_path": "C:\\definitely-not-a-picoclaw.zip",
                "expected_state_version": action_state_version()
            }),
        )
        .expect_err("arbitrary temporary directories are never USB tool-disk targets");
        assert!(
            error.contains("invalid_target"),
            "non-removable root was not rejected: {error}"
        );
    }
    #[test]
    fn target_identity_must_match_the_displayed_root() {
        let expected = PortableTarget {
            id: "windows:volume-a:1234".into(), root: PathBuf::from("F:\\"),
            label: "KING".into(), filesystem: "exFAT".into(), total_bytes: 1, free_bytes: 1, read_only: false,
        };
        assert!(target_from_inventory(&json!({"target_id":"windows:volume-a:1234","target_root":"F:\\"}), &[expected.clone()]).is_ok());
        assert!(target_from_inventory(&json!({"target_id":"windows:volume-a:1234","target_root":"G:\\"}), &[expected]).unwrap_err().contains("invalid_target"));
    }
    #[test]
    fn executable_location_selects_its_own_removable_target_not_scan_order() {
        let f = PortableTarget { id: "windows:f".into(), root: PathBuf::from("F:\\"), label: "FIRST".into(), filesystem: "exFAT".into(), total_bytes: 1, free_bytes: 1, read_only: false };
        let e = PortableTarget { id: "windows:e".into(), root: PathBuf::from("E:\\"), label: "SELF".into(), filesystem: "NTFS".into(), total_bytes: 1, free_bytes: 1, read_only: false };
        assert_eq!(launched_from_target_id(&[f, e], Path::new("e:\\U-King.exe")), Some("windows:e".into()));
        assert_eq!(launched_from_target_id(&[], Path::new("e:\\U-King.exe")), None);
    }
    #[test]
    fn none_credential_plan_preserves_an_existing_credential() {
        let p = root();
        fs::create_dir_all(data(&p)).unwrap();
        fs::write(data(&p).join(".security.yml"), "example").unwrap();
        let (replacement, mode) = credential_plan(&p, "none", None).unwrap();
        assert!(replacement.is_none());
        assert_eq!(mode, "preserved_existing");
        assert!(data(&p).join(".security.yml").is_file());
        assert!(credential_plan(&p, "provider:not-ready", None).unwrap_err().contains("invalid_input"));
        let _ = fs::remove_dir_all(p);
    }
    #[test]
    fn ownership_preflight_refuses_unknown_same_name_content() {
        let p = root();
        fs::create_dir_all(genie(&p)).unwrap();
        fs::write(genie(&p).join("someone-elses-file.txt"), "do not touch").unwrap();
        assert!(preflight_ownership(&p).unwrap_err().contains("target_conflict"));
        fs::remove_dir_all(&p).unwrap();

        fs::create_dir_all(genie(&p)).unwrap();
        write_install_marker(&p).unwrap();
        assert!(preflight_ownership(&p).is_ok());
        let _ = fs::remove_dir_all(p);
    }
    #[test]
    fn runtime_validation_rejects_partial_staging_directories() {
        let p = root();
        fs::create_dir_all(&p).unwrap();
        fs::write(p.join("picoclaw.exe"), "not a runtime").unwrap();
        assert!(runtime_files_are_valid(&p).unwrap_err().contains("LICENSE"));
        let _ = fs::remove_dir_all(p);
    }
    #[test]
    fn recorded_artifact_hashes_detect_launcher_tampering() {
        let p = root();
        fs::create_dir_all(current(&p)).unwrap();
        for name in ["picoclaw.exe", "LICENSE", "README.md"] { fs::write(current(&p).join(name), name).unwrap(); }
        fs::write(launcher(&p), "known launcher").unwrap();
        let hashes = artifact_hashes(&p).unwrap();
        assert!(artifacts_match(&p, &hashes));
        fs::write(launcher(&p), "changed launcher").unwrap();
        assert!(!artifacts_match(&p, &hashes));
        let _ = fs::remove_dir_all(p);
    }
    #[test]
    fn picoclaw_agent_busy_is_none_without_running_agent() {
        // 判活契约：只有「从本目标 runtime 目录启动的 picoclaw 进程」才算 busy。
        // 测试环境没有 picoclaw.exe 进程（宿主机进程表按名字过滤即空），
        // 任何临时目录都应返回 None —— 不会误报、不会全局按镜像名命中。
        let p = root();
        assert_eq!(picoclaw_agent_busy(&p), None);
    }
    #[test]
    fn deploy_preflights_space_before_touching_disk() {
        assert!(ensure_space(MIN_DEPLOY_FREE_BYTES).is_ok());
        assert!(ensure_space(MIN_DEPLOY_FREE_BYTES + 1).is_ok());
        let error = ensure_space(MIN_DEPLOY_FREE_BYTES - 1).unwrap_err();
        assert!(error.contains("insufficient_space"), "{error}");
        assert!(error.contains("未写入任何文件"), "{error}");
    }
}
