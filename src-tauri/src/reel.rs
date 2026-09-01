//! 一键成片的本地壳：参数/历史/进度由 Rust 管，真正的分镜、出图、视频、TTS、拼接全部复用
//! `skills/aigc/scripts/gen-reel.mjs`。本模块不复制任何生成流程。

use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;

const MAX_ITEMS: usize = 12;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReelParams {
    #[serde(default)] pub prompt: String,
    #[serde(default)] pub storyboard: Option<String>,
    #[serde(default)] pub shots: Vec<String>,
    #[serde(default)] pub narration: Option<String>,
    #[serde(default)] pub voice: Option<String>,
    #[serde(default)] pub bgm_prompt: Option<String>,
    #[serde(default)] pub resolution: Option<String>,
    /// M2 创作预设预留。接 presets 前必须校验 schema_version=1，不能盲透服务端数据。
    #[serde(default)] pub preset_id: Option<String>,
    /// 仅由 prepare_params 写入，并随历史落盘。重跑不重新解读后来可能变更的预设定义。
    #[serde(default)] pub style_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReelPresetOut {
    pub schema_version: u8,
    pub id: &'static str,
    pub title: &'static str,
    pub description: &'static str,
}
struct ReelPreset { out: ReelPresetOut, style_hint: &'static str }
const REEL_PRESETS: &[ReelPreset] = &[
    ReelPreset { out: ReelPresetOut { schema_version: 1, id: "cinematic-story", title: "电影叙事", description: "镜头推进 · 光影层次 · 适合故事感短片" }, style_hint: "电影感叙事，光影层次丰富，镜头平稳推进" },
    ReelPreset { out: ReelPresetOut { schema_version: 1, id: "product-showcase", title: "产品展示", description: "主体特写 · 干净布光 · 适合商品和功能演示" }, style_hint: "产品特写，干净商业布光，镜头缓慢环绕展示主体" },
    ReelPreset { out: ReelPresetOut { schema_version: 1, id: "social-short", title: "社媒短片", description: "节奏明确 · 视觉醒目 · 适合活动与口播素材" }, style_hint: "短视频节奏，主体醒目，画面有明确视觉焦点" },
];
fn preset_by_id(id: &str) -> Option<&'static ReelPreset> { REEL_PRESETS.iter().find(|preset| preset.out.id == id) }
pub fn list_presets() -> Vec<ReelPresetOut> { REEL_PRESETS.iter().map(|preset| preset.out.clone()).collect() }

/// 预设目录为 schema v1、后端白名单。前端只交 id，不能把任意 JSON/风格提示透传到脚本。
/// BGM 是可能收费的独立通道，必须保持前端显式开关的语义，预设不得暗中补上。
pub fn prepare_params(mut params: ReelParams) -> Result<ReelParams, String> {
    params.style_hint = None; // 不信 UI 传来的内部展开字段。
    if let Some(id) = params.preset_id.as_deref() {
        let preset = preset_by_id(id).ok_or_else(|| "创作预设已失效，请重新选择".to_string())?;
        params.style_hint = Some(preset.style_hint.into());
    }
    validate(&params)?;
    Ok(params)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReelRecord {
    pub id: i64,
    #[serde(default)] pub prompt: String,
    #[serde(default)] pub storyboard: Option<String>,
    #[serde(default)] pub shots: Vec<String>,
    #[serde(default)] pub narration: Option<String>,
    #[serde(default)] pub voice: Option<String>,
    #[serde(default)] pub bgm_prompt: Option<String>,
    #[serde(default)] pub resolution: Option<String>,
    #[serde(default)] pub preset_id: Option<String>,
    #[serde(default)] pub style_hint: Option<String>,
    #[serde(default)] pub file: Option<String>,
    #[serde(default = "running")] pub status: String,
    #[serde(default)] pub error: Option<String>,
    #[serde(default)] pub degraded: bool,
    #[serde(default)] pub warnings: Vec<String>,
    #[serde(default)] pub ts: i64,
}
fn running() -> String { "running".into() }

#[derive(Debug, Clone, Serialize)]
pub struct ReelItemOut {
    pub id: i64,
    pub prompt: String,
    pub shots: Vec<String>,
    pub narration: Option<String>,
    pub voice: Option<String>,
    pub bgm_prompt: Option<String>,
    pub resolution: Option<String>,
    pub preset_id: Option<String>,
    pub status: String,
    pub have_video: bool,
    pub error: Option<String>,
    pub degraded: bool,
    pub warnings: Vec<String>,
    pub ts: i64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct HistoryFile { #[serde(default)] version: u32, #[serde(default)] items: Vec<ReelRecord> }

fn home_dir() -> PathBuf { crate::installer::user_home_dir() }
fn uking_home() -> PathBuf { home_dir().join(".uking") }
fn reel_dir() -> PathBuf { uking_home().join("reel") }
/// 成片和普通 AI 视频共用 video/，与现有 asset scope 保持单一事实，不新增 scope。
fn video_dir() -> PathBuf { uking_home().join("video") }
fn history_path() -> PathBuf { reel_dir().join("history.json") }
fn now_ms() -> i64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0) }

fn read_file() -> HistoryFile {
    std::fs::read_to_string(history_path()).ok().and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(HistoryFile { version: 1, items: vec![] })
}
fn write_file(file: &HistoryFile) -> Result<(), String> {
    std::fs::create_dir_all(reel_dir()).map_err(|e| format!("创建成片历史目录失败: {e}"))?;
    std::fs::write(history_path(), serde_json::to_vec_pretty(file).map_err(|e| e.to_string())?)
        .map_err(|e| format!("写入成片历史失败: {e}"))
}
fn prune(file: &mut HistoryFile) {
    while file.items.len() > MAX_ITEMS {
        if let Some(old) = file.items.pop() {
            if let Some(name) = old.file { let _ = std::fs::remove_file(video_dir().join(name)); }
        }
    }
}
fn record_to_params(r: &ReelRecord) -> ReelParams {
    ReelParams { prompt: r.prompt.clone(), storyboard: r.storyboard.clone(), shots: r.shots.clone(), narration: r.narration.clone(), voice: r.voice.clone(), bgm_prompt: r.bgm_prompt.clone(), resolution: r.resolution.clone(), preset_id: r.preset_id.clone(), style_hint: r.style_hint.clone() }
}
fn validate(params: &ReelParams) -> Result<(), String> {
    if params.prompt.chars().count() > 4_000 || params.storyboard.as_deref().is_some_and(|s| s.chars().count() > 8_000) { return Err("创作文本过长，请缩短后再试".into()); }
    if params.shots.len() > 8 || params.shots.iter().any(|shot| shot.chars().count() > 1_000) { return Err("分镜最多 8 条且每条不超过 1000 字".into()); }
    if [params.narration.as_deref(), params.bgm_prompt.as_deref()].into_iter().flatten().any(|s| s.chars().count() > 2_000) { return Err("旁白或 BGM 描述不能超过 2000 字".into()); }
    if params.voice.as_deref().is_some_and(|s| s.chars().count() > 64) { return Err("音色标识过长".into()); }
    if params.resolution.as_deref().is_some_and(|s| s != "480p" && s != "720p") { return Err("仅支持 480p 或 720p".into()); }
    if params.storyboard.as_deref().is_some_and(|s| !s.trim().is_empty()) || !params.shots.is_empty() { return Ok(()); }
    if params.prompt.trim().is_empty() { return Err("请先写一个起手提示词或提供分镜脚本".into()); }
    Ok(())
}

pub fn create_record(params: &ReelParams) -> Result<i64, String> {
    validate(params)?;
    let id = now_ms();
    let mut file = read_file();
    file.items.insert(0, ReelRecord { id, prompt: params.prompt.clone(), storyboard: params.storyboard.clone(), shots: params.shots.clone(), narration: params.narration.clone(), voice: params.voice.clone(), bgm_prompt: params.bgm_prompt.clone(), resolution: params.resolution.clone(), preset_id: params.preset_id.clone(), style_hint: params.style_hint.clone(), file: None, status: "running".into(), error: None, degraded: false, warnings: vec![], ts: id });
    prune(&mut file); file.version = 1; write_file(&file)?; Ok(id)
}
fn update(id: i64, status: &str, file_name: Option<String>, error: Option<String>, degraded: bool, warnings: Vec<String>) {
    let mut file = read_file();
    if let Some(r) = file.items.iter_mut().find(|r| r.id == id) {
        r.status = status.into(); r.file = file_name; r.error = error; r.degraded = degraded; r.warnings = warnings;
    }
    let _ = write_file(&file);
}
/// 壳层启动失败/被中断时也要把 running 留痕改成 failed，避免客户误以为还在后台生成。
pub fn mark_failed(id: i64, error: impl Into<String>) { update(id, "failed", None, Some(error.into()), false, vec![]); }
pub fn list_history() -> Vec<ReelItemOut> {
    read_file().items.into_iter().map(|r| {
        let have_video = r.file.as_ref().is_some_and(|n| video_dir().join(n).is_file());
        ReelItemOut { id: r.id, prompt: r.prompt, shots: r.shots, narration: r.narration, voice: r.voice, bgm_prompt: r.bgm_prompt, resolution: r.resolution, preset_id: r.preset_id, status: r.status, have_video, error: r.error, degraded: r.degraded, warnings: r.warnings, ts: r.ts }
    }).collect()
}
pub fn file_path(id: i64) -> Option<PathBuf> { read_file().items.into_iter().find(|r| r.id == id).and_then(|r| r.file).map(|n| video_dir().join(n)).filter(|p| p.is_file()) }
pub fn params_for_regeneration(id: i64) -> Result<ReelParams, String> {
    let r = read_file().items.into_iter().find(|r| r.id == id).ok_or("找不到该成片任务")?;
    if r.status != "running" && r.status != "failed" { return Err("只有未完成或失败的任务可以重新生成".into()); }
    Ok(record_to_params(&r))
}
pub fn restart_record(id: i64) -> Result<ReelParams, String> {
    let params = params_for_regeneration(id)?;
    update(id, "running", None, None, false, vec![]);
    Ok(params)
}
pub fn delete_record(id: i64) -> Result<(), String> {
    let mut file = read_file();
    if let Some(pos) = file.items.iter().position(|r| r.id == id) {
        if let Some(name) = file.items.remove(pos).file { let _ = std::fs::remove_file(video_dir().join(name)); }
        write_file(&file)?;
    }
    Ok(())
}

fn node_path() -> Result<PathBuf, String> {
    if let Some(p) = std::env::var_os("UKING_NODE_PATH").map(PathBuf::from).filter(|p| p.is_file()) { return Ok(p); }
    let exe = std::env::current_exe().map_err(|e| format!("无法定位 U-King 程序: {e}"))?;
    let base = exe.parent().unwrap_or(Path::new("."));
    let names: &[&str] = if cfg!(windows) { &["node.exe", "node"] } else { &["node"] };
    for root in [base.join("runtime/node-win-x64"), base.join("runtime/node"), base.join("../runtime/node-win-x64"), base.join("../runtime/node") ] {
        for name in names { let p = root.join(name); if p.is_file() { return Ok(p); } }
    }
    // 不把绝对路径写死；最后交给客户已经配置好的 PATH。
    Ok(PathBuf::from(if cfg!(windows) { "node.exe" } else { "node" }))
}
/// Skillpack 导出后的目录名是 `uking-aigc`，而早期内嵌/开发版使用 `aigc`。
/// 两种布局都要识别：绿色版首次解包后通常只有前者。
fn skillpack_reel_script(root: &Path) -> Option<PathBuf> {
    [
        root.join("uking-aigc/scripts/gen-reel.mjs"),
        root.join("aigc/scripts/gen-reel.mjs"),
    ].into_iter().find(|path| path.is_file())
}

fn reel_script() -> Result<PathBuf, String> {
    if let Some(root) = std::env::var_os("UKING_SKILLS_DIR") {
        if let Some(path) = skillpack_reel_script(&PathBuf::from(root)) { return Ok(path); }
    }
    // `skillpack::ensure_skillpack()` 导出的实际位置。不能只依赖 exe 旁的开发目录，
    // 否则绿色版在客户机上会报“找不到内置 gen-reel.mjs”。
    if let Some(path) = skillpack_reel_script(&uking_home().join("skills")) { return Ok(path); }
    let exe = std::env::current_exe().map_err(|e| format!("无法定位 U-King 程序: {e}"))?;
    let base = exe.parent().unwrap_or(Path::new("."));
    for p in [
        base.join("skills/uking-aigc/scripts/gen-reel.mjs"),
        base.join("skills/aigc/scripts/gen-reel.mjs"),
        base.join("../skills/uking-aigc/scripts/gen-reel.mjs"),
        base.join("../skills/aigc/scripts/gen-reel.mjs"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("skills/aigc/scripts/gen-reel.mjs"),
    ] { if p.is_file() { return Ok(p); } }
    Err("找不到内置 gen-reel.mjs；请在工具中心重新安装/修复 U-King".into())
}
fn with_ffmpeg_path(cmd: &mut Command) {
    let ff = uking_home().join("tools/ffmpeg");
    if !ff.is_dir() { return; }
    let mut paths = vec![ff];
    if let Some(old) = std::env::var_os("PATH") { paths.extend(std::env::split_paths(&old)); }
    if let Ok(path) = std::env::join_paths(paths) { cmd.env("PATH", path); }
}
fn ffmpeg_ready() -> Result<(), String> {
    let mut cmd = Command::new("ffmpeg"); with_ffmpeg_path(&mut cmd);
    match cmd.arg("-version").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status() {
        Ok(s) if s.success() => Ok(()),
        _ => Err("未安装 ffmpeg，一键成片需要它；请在工具箱或安装向导安装后再试".into()),
    }
}
fn args_for(params: &ReelParams, out: &Path) -> Vec<String> {
    let mut args = vec!["--out".into(), out.display().to_string(), "--json".into()];
    if let Some(s) = params.storyboard.as_deref().filter(|s| !s.trim().is_empty()) { args.extend(["--storyboard".into(), s.into()]); }
    else if !params.shots.is_empty() { for s in &params.shots { args.extend(["--shot".into(), s.clone()]); } }
    else { args.extend(["--shot".into(), format!("{}::{}", params.prompt.trim(), params.style_hint.as_deref().unwrap_or("镜头缓慢推进"))]); }
    if let Some(s) = params.narration.as_deref().filter(|s| !s.trim().is_empty()) { args.extend(["--narration".into(), s.into()]); }
    if let Some(s) = params.voice.as_deref().filter(|s| !s.trim().is_empty()) { args.extend(["--voice".into(), s.into()]); }
    if let Some(s) = params.bgm_prompt.as_deref().filter(|s| !s.trim().is_empty()) { args.extend(["--bgm-prompt".into(), s.into()]); }
    if let Some(s) = params.resolution.as_deref().filter(|s| !s.trim().is_empty()) { args.extend(["--resolution".into(), s.into()]); }
    args
}
fn phase(detail: &str) -> &str {
    if detail.contains("【1/5") { "dialogue" } else if detail.contains("【2/5") { "storyboard" } else if detail.contains("【3/5") { "video" } else if detail.contains("【4/5") { "voice" } else if detail.contains("【5/5") { "stitch" } else { "running" }
}
#[derive(Deserialize, Default)] struct ScriptOut { #[serde(default)] ok: bool, #[serde(default)] degraded: bool, #[serde(default)] warnings: Vec<String>, #[serde(default)] error: String }

/// 执行一个已有记录。stderr 每行实时回调；结束时只信 stdout 的最后一个 JSON 行。
pub fn run(id: i64, params: &ReelParams, key: &str, on_progress: &dyn Fn(&str, &str)) -> Result<(), String> {
    validate(params)?;
    if let Err(e) = ffmpeg_ready() { mark_failed(id, e.clone()); return Err(e); }
    std::fs::create_dir_all(video_dir()).map_err(|e| format!("创建视频目录失败: {e}"))?;
    let name = format!("reel-{id}.mp4"); let out = video_dir().join(&name);
    let mut args = args_for(params, &out); args.extend(["--key".into(), key.into()]);
    let mut command = Command::new(node_path()?); command.arg(reel_script()?).args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()); with_ffmpeg_path(&mut command);
    let mut child = command.spawn().map_err(|e| format!("启动一键成片失败: {e}"))?;
    let stderr = child.stderr.take().ok_or("无法读取成片进度")?;
    let stdout = child.stdout.take().ok_or("无法读取成片结果")?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || { for line in BufReader::new(stderr).lines().map_while(Result::ok) { let _ = tx.send(line); } });
    let out_thread = std::thread::spawn(move || BufReader::new(stdout).lines().filter_map(Result::ok).collect::<Vec<_>>());
    // 不能只等第一条日志：生成视频常有数十秒静默期。轮询子进程状态期间持续抽 stderr，
    // 才能把后续【n/5】阶段即时送到前端。
    let status = loop {
        match rx.recv_timeout(std::time::Duration::from_millis(150)) {
            Ok(line) => on_progress(phase(&line), &line),
            Err(mpsc::RecvTimeoutError::Disconnected) | Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if let Some(status) = child.try_wait().map_err(|e| format!("等待一键成片结束失败: {e}"))? { break status; }
    };
    let lines = out_thread.join().unwrap_or_default();
    let result = lines.last().and_then(|s| serde_json::from_str::<ScriptOut>(s).ok()).unwrap_or_default();
    if !status.success() || !result.ok {
        let error = if result.error.is_empty() { format!("一键成片失败（退出码 {:?}）", status.code()) } else { result.error };
        update(id, "failed", None, Some(error.clone()), false, result.warnings); return Err(error);
    }
    if !out.is_file() { let error = "一键成片返回成功，但没有找到 mp4 成片".to_string(); update(id, "failed", None, Some(error.clone()), false, result.warnings); return Err(error); }
    let status = if result.degraded { "degraded" } else { "done" };
    update(id, status, Some(name), None, result.degraded, result.warnings); Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn params_and_recreate_are_persisted() {
        let _sb = crate::testsandbox::enter("reel-history", &[".uking"]);
        let p = ReelParams { prompt: "霓虹街道".into(), shots: vec!["霓虹街道::推进".into()], narration: Some("欢迎来到未来".into()), voice: Some("Cherry".into()), bgm_prompt: Some("电子乐".into()), resolution: Some("480p".into()), ..Default::default() };
        let id = create_record(&p).unwrap(); let restored = restart_record(id).unwrap();
        assert_eq!(restored.shots, p.shots); assert_eq!(restored.narration, p.narration); assert_eq!(list_history()[0].status, "running");
    }
    #[test] fn prune_keeps_twelve_newest() {
        let mut f = HistoryFile { version: 1, items: (0..13).map(|n| ReelRecord { id:n, prompt:String::new(), storyboard:None, shots:vec![], narration:None, voice:None, bgm_prompt:None, resolution:None, preset_id:None, style_hint:None, file:None, status:"done".into(), error:None, degraded:false, warnings:vec![], ts:n }).collect() };
        prune(&mut f); assert_eq!(f.items.len(), MAX_ITEMS);
    }
    #[test] fn preset_is_whitelisted_and_expanded_before_history() {
        let p = prepare_params(ReelParams { prompt: "新品耳机".into(), preset_id: Some("product-showcase".into()), ..Default::default() }).unwrap();
        assert!(p.bgm_prompt.is_none());
        assert!(p.style_hint.as_deref().is_some_and(|hint| hint.contains("产品特写")));
        assert!(prepare_params(ReelParams { prompt: "x".into(), preset_id: Some("untrusted".into()), ..Default::default() }).is_err());
    }
    #[test] fn preset_never_adds_or_overwrites_bgm_and_replays_style() {
        let _sb = crate::testsandbox::enter("reel-preset-replay", &[".uking"]);
        let p = prepare_params(ReelParams { prompt: "新品耳机".into(), preset_id: Some("product-showcase".into()), bgm_prompt: Some("客户自己的音乐".into()), ..Default::default() }).unwrap();
        assert_eq!(p.bgm_prompt.as_deref(), Some("客户自己的音乐"));
        let id = create_record(&p).unwrap();
        assert_eq!(restart_record(id).unwrap().style_hint, p.style_hint);
    }
    #[test] fn skillpack_export_layout_resolves_reel_script() {
        let root = std::env::temp_dir().join(format!("uking-reel-script-test-{}", now_ms()));
        let script = root.join("uking-aigc/scripts/gen-reel.mjs");
        std::fs::create_dir_all(script.parent().unwrap()).unwrap();
        std::fs::write(&script, "// test").unwrap();
        assert_eq!(skillpack_reel_script(&root), Some(script));
        // Windows Defender/索引器可能短暂占用刚创建的临时目录；清理失败不应把
        // 路径解析的回归测试误报为失败。
        let _ = std::fs::remove_dir_all(root);
    }
}
