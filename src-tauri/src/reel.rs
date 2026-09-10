//! 一键成片的本地壳：参数/历史/进度由 Rust 管，真正的分镜、出图、视频、TTS、拼接全部复用
//! `skills/aigc/scripts/gen-reel.mjs`。本模块不复制任何生成流程。

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::sync::{Mutex, OnceLock};

const MAX_ITEMS: usize = 12;

// 一条成片会启动一个跑到底的 gen-reel.mjs 子进程（内部按次付费）。App 启动恢复与
// 页面/Action 重复提交可能同时撞到同一条记录，仿 video.rs：本地只许一个跑者，
// 否则两个线程会同时写同一个 mp4、互相覆盖失败/成功的落盘结果。
static ACTIVE_RUNS: OnceLock<Mutex<HashSet<i64>>> = OnceLock::new();

pub fn try_begin_run(id: i64) -> bool {
    ACTIVE_RUNS.get_or_init(|| Mutex::new(HashSet::new())).lock().map(|mut ids| ids.insert(id)).unwrap_or(false)
}
pub fn end_run(id: i64) {
    if let Ok(mut ids) = ACTIVE_RUNS.get_or_init(|| Mutex::new(HashSet::new())).lock() { ids.remove(&id); }
}
fn is_active_run(id: i64) -> bool {
    ACTIVE_RUNS.get_or_init(|| Mutex::new(HashSet::new())).lock().map(|ids| ids.contains(&id)).unwrap_or(false)
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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
    /// 客户主动保留的项目资产：不受历史裁剪影响（既不从历史移除，也不删磁盘文件）。
    #[serde(default)] pub kept: bool,
    /// `kept=true` 后对应的 `~/.uking/projects/<project_id>/` 目录。
    #[serde(default)] pub project_id: Option<String>,
    /// 预留：哪个供应商出的这条成片。当前只有虾盘云，`None` 视作虾盘云；只加字段不加路由。
    #[serde(default)] pub provider: Option<String>,
    /// 两段式提交下用于轮询的当前阶段（dialogue/storyboard/video/voice/stitch），终态清空。
    #[serde(default)] pub phase: Option<String>,
    /// 与 `phase` 配套的人类可读进度文案，终态清空。
    #[serde(default)] pub detail: Option<String>,
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
    pub kept: bool,
    pub project_id: Option<String>,
    pub provider: Option<String>,
    pub phase: Option<String>,
    pub detail: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct HistoryFile { #[serde(default)] version: u32, #[serde(default)] items: Vec<ReelRecord> }

fn home_dir() -> PathBuf { crate::installer::user_home_dir() }
fn uking_home() -> PathBuf { home_dir().join(".uking") }
fn reel_dir() -> PathBuf { uking_home().join("reel") }
/// 成片和普通 AI 视频共用 video/，与现有 asset scope 保持单一事实，不新增 scope。
fn video_dir() -> PathBuf { uking_home().join("video") }
fn history_path() -> PathBuf { reel_dir().join("history.json") }
/// 项目资产根：与 `video_dir()`（缓存/最近历史）严格分开的第三层——只有客户主动删，
/// 任何历史裁剪都不许碰。`kept()` 把成片拷进这里后，两份文件同时存在（缓存那份仍按
/// 普通规则可被覆盖/清理，项目资产这份不会）。
fn projects_dir() -> PathBuf { uking_home().join("projects") }
fn project_dir(project_id: &str) -> PathBuf { projects_dir().join(project_id) }
fn project_export_dir(project_id: &str) -> PathBuf { project_dir(project_id).join("exports") }
fn project_manifest_path(project_id: &str) -> PathBuf { project_dir(project_id).join("manifest.json") }
fn pending_submit_path() -> PathBuf { reel_dir().join("pending-submit.json") }
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
/// 只裁剪**未保留**的记录，且只按「最老的未保留」逐个删；`kept=true` 的记录既不计入
/// `MAX_ITEMS` 上限、也绝不删它的文件——它已经从「最近历史」升格成「项目资产」。
fn prune(file: &mut HistoryFile) {
    loop {
        let non_kept = file.items.iter().filter(|r| !r.kept).count();
        if non_kept <= MAX_ITEMS { break; }
        let Some(pos) = file.items.iter().rposition(|r| !r.kept) else { break; };
        let old = file.items.remove(pos);
        if let Some(name) = old.file { let _ = std::fs::remove_file(video_dir().join(name)); }
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
    file.items.insert(0, ReelRecord { id, prompt: params.prompt.clone(), storyboard: params.storyboard.clone(), shots: params.shots.clone(), narration: params.narration.clone(), voice: params.voice.clone(), bgm_prompt: params.bgm_prompt.clone(), resolution: params.resolution.clone(), preset_id: params.preset_id.clone(), style_hint: params.style_hint.clone(), file: None, status: "running".into(), error: None, degraded: false, warnings: vec![], ts: id, kept: false, project_id: None, provider: None, phase: None, detail: None });
    prune(&mut file); file.version = 1; write_file(&file)?; Ok(id)
}
fn update(id: i64, status: &str, file_name: Option<String>, error: Option<String>, degraded: bool, warnings: Vec<String>) {
    let mut file = read_file();
    if let Some(r) = file.items.iter_mut().find(|r| r.id == id) {
        r.status = status.into(); r.file = file_name; r.error = error; r.degraded = degraded; r.warnings = warnings;
        // 状态变更（含重投重置为 running）都代表一次新的阶段，旧的两段式进度文案必须清掉，
        // 否则「上一次卡在第 3/5 步」的文案会在下一次生成时误留在界面上。
        r.phase = None; r.detail = None;
    }
    let _ = write_file(&file);
}
/// 两段式提交下用于轮询的进度更新，只碰 `phase`/`detail`，不动状态机字段。
fn set_progress(id: i64, phase: &str, detail: &str) {
    let mut file = read_file();
    if let Some(r) = file.items.iter_mut().find(|r| r.id == id) {
        r.phase = Some(phase.into()); r.detail = Some(detail.into());
    }
    let _ = write_file(&file);
}
/// 壳层启动失败/被中断时也要把 running 留痕改成 failed，避免客户误以为还在后台生成。
pub fn mark_failed(id: i64, error: impl Into<String>) { update(id, "failed", None, Some(error.into()), false, vec![]); }
pub fn list_history() -> Vec<ReelItemOut> {
    read_file().items.into_iter().map(|r| {
        let have_video = r.file.as_ref().is_some_and(|n| video_dir().join(n).is_file());
        ReelItemOut { id: r.id, prompt: r.prompt, shots: r.shots, narration: r.narration, voice: r.voice, bgm_prompt: r.bgm_prompt, resolution: r.resolution, preset_id: r.preset_id, status: r.status, have_video, error: r.error, degraded: r.degraded, warnings: r.warnings, ts: r.ts, kept: r.kept, project_id: r.project_id, provider: r.provider, phase: r.phase, detail: r.detail }
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

// ── 项目资产：客户主动保留，历史裁剪永不触碰 ──────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectManifest {
    pub project_id: String,
    pub source: &'static str,
    pub reel_id: i64,
    pub prompt: String,
    pub preset_id: Option<String>,
    pub resolution: Option<String>,
    pub provider: Option<String>,
    pub file: String,
    /// "charged"：这条已经是成功出片、已计费的成片，拷进项目资产不产生新费用。
    pub cost_status: &'static str,
    pub ts: i64,
}

/// 把一条已完成的成片标记为项目资产：拷贝 mp4 到 `projects/<id>/exports/`、写 manifest、
/// 记 `kept=true`。**幂等**：已经保留过再调一次直接返回成功，不重复拷贝。
pub fn keep_record(id: i64) -> Result<(), String> {
    let mut file = read_file();
    let idx = file.items.iter().position(|r| r.id == id).ok_or("找不到该成片任务")?;
    if file.items[idx].kept { return Ok(()); }
    let name = file.items[idx].file.clone().ok_or("该成片尚未生成完成，暂时无法保留")?;
    let src = video_dir().join(&name);
    if !src.is_file() { return Err("成片文件不存在，无法保留".into()); }
    let project_id = format!("reel-{id}");
    let export_dir = project_export_dir(&project_id);
    std::fs::create_dir_all(&export_dir).map_err(|e| format!("创建项目目录失败: {e}"))?;
    std::fs::copy(&src, export_dir.join(&name)).map_err(|e| format!("拷贝成片到项目目录失败: {e}"))?;
    let manifest = ProjectManifest {
        project_id: project_id.clone(),
        source: "reel",
        reel_id: id,
        prompt: file.items[idx].prompt.clone(),
        preset_id: file.items[idx].preset_id.clone(),
        resolution: file.items[idx].resolution.clone(),
        provider: file.items[idx].provider.clone(),
        file: name,
        cost_status: "charged",
        ts: now_ms(),
    };
    std::fs::write(project_manifest_path(&project_id), serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?)
        .map_err(|e| format!("写入项目 manifest 失败: {e}"))?;
    file.items[idx].kept = true;
    file.items[idx].project_id = Some(project_id);
    write_file(&file)
}

// ── 幂等提交事务：进程死在“已经起了付费子进程、不知道跑到哪一步”的缝里时兜底 ──

/// 写前日志：提交前落盘，成功/失败终态后清除。跟 `video.rs::PendingSubmit` 同一形态，
/// 但语义不同——video 的缝是「POST 到服务端但没收到 task_id」，reel 的缝是
/// 「gen-reel.mjs 子进程已经在跑（内部按次付费），但本机不知道它跑到第几步就被杀」。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingSubmit { pub request_id: String, pub id: i64, pub ts: i64 }

fn stage_pending(id: i64, request_id: Option<&str>) -> Result<String, String> {
    let seed = format!("{}|{}|reel|{id}", now_ms(), std::process::id());
    let request_id = request_id.filter(|s| !s.trim().is_empty()).map(str::to_string)
        .unwrap_or_else(|| format!("ukr1-{}", &blake3::hash(seed.as_bytes()).to_hex()[..32]));
    if request_id.len() > 160 || !request_id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')) {
        return Err("invalid_input: request_id 只能包含 ASCII 字母、数字、-、_、.".into());
    }
    let pending = PendingSubmit { request_id: request_id.clone(), id, ts: now_ms() };
    std::fs::create_dir_all(reel_dir()).map_err(|e| format!("创建成片历史目录失败: {e}"))?;
    let tmp = pending_submit_path().with_extension(format!("json.{}.tmp", std::process::id()));
    std::fs::write(&tmp, serde_json::to_vec_pretty(&pending).map_err(|e| e.to_string())?)
        .map_err(|e| format!("保存成片提交事务失败: {e}"))?;
    std::fs::rename(&tmp, pending_submit_path()).map_err(|e| format!("提交事务落盘失败: {e}"))?;
    Ok(request_id)
}
pub fn pending_submit() -> Option<PendingSubmit> {
    std::fs::read_to_string(pending_submit_path()).ok().and_then(|s| serde_json::from_str(&s).ok())
}
fn clear_pending_submit(id: i64) {
    if pending_submit().is_some_and(|p| p.id == id) { let _ = std::fs::remove_file(pending_submit_path()); }
}

/// 启动时 / 客户点"查一次"时调用：残留 pending 若对应的本地跑者确实已经不在了（不在
/// `ACTIVE_RUNS` 里），说明进程死在了缝里，把它从 `running` 改判 `pending-verify`——
/// **绝不自动重投**，只留痕等客户显式决定。如果那条其实还在本进程里正常跑（比如
/// 这个调用先于后台线程完成前触发），原样跳过，不误伤真正在跑的任务。
pub fn recover_dangling_pending() {
    let Some(p) = pending_submit() else { return; };
    if is_active_run(p.id) { return; }
    let mut file = read_file();
    if let Some(r) = file.items.iter_mut().find(|r| r.id == p.id) {
        if r.status == "running" { r.status = "pending-verify".into(); let _ = write_file(&file); }
    }
}
/// "查一次"：重新核实一遍本地跑者是否还在（成片没有独立可查询的远端任务 id，
/// 核实范围仅限本地）。返回核实后的最新状态，不做任何重投。
pub fn recheck_pending(id: i64) -> Result<String, String> {
    recover_dangling_pending();
    let file = read_file();
    let r = file.items.iter().find(|r| r.id == id).ok_or("找不到该成片任务")?;
    Ok(r.status.clone())
}
/// 只做状态转换（`pending-verify` → `running`），不碰子进程；拆开是为了让这段
/// 判定逻辑本身可以在不牵动后台线程的情况下被直接测到。
fn reopen_pending_verify_for_resubmit(id: i64) -> Result<ReelParams, String> {
    let file = read_file();
    let r = file.items.iter().find(|r| r.id == id).ok_or("找不到该成片任务")?;
    if r.status != "pending-verify" { return Err("只有“提交结果未知”的任务才能用这个入口重投".into()); }
    let params = record_to_params(r);
    clear_pending_submit(id);
    update(id, "running", None, None, false, vec![]);
    Ok(params)
}
/// "我确认没扣费，重投"：唯一允许把 `pending-verify` 任务重新送去生成的入口，
/// 必须由客户显式调用，不会被任何轮询/启动逻辑自动触发。
pub fn confirm_no_charge_and_resubmit(id: i64, key: String) -> Result<i64, String> {
    let params = reopen_pending_verify_for_resubmit(id)?;
    stage_pending(id, None)?;
    spawn_generation(id, params, key);
    Ok(id)
}

/// 两段式提交核心：落一条新记录 + 写前日志，然后把真正的生成扔进后台线程，**立即返回**。
/// GUI（走 Action）与 CLI/MCP 都经这里；不会阻塞调用方到生成完成，避免外部 AI 工具
/// 因为 40 镜的长任务超时后误判失败重投、重复烧钱。
///
/// 同一 `request_id` 重放：若上一条提交事务仍未清空（可能仍在跑，也可能进程死在缝里
/// 还没被 `recover_dangling_pending` 接管），且参数相同则直接回原记录 id，不再起第二个
/// 付费子进程；参数不同则明确拒绝，而不是静默排队。
/// `key` 由调用方（组合根 `lib.rs`）取得后传入，`reel.rs` 本身不认识 `device` 模块——
/// 这条边如果长在 reel 内部会被 `check-module-coupling.mjs` 拦（脚本自己给的改法就是
/// 「组合根去问、把结果当参数传进来」），所以设备鉴权在调用侧同步取一次即可，
/// 生成本身用不到设备模块的其它能力。
pub fn submit_start(params: ReelParams, request_id: Option<&str>, key: String) -> Result<i64, String> {
    let params = prepare_params(params)?;
    if let Some(old) = pending_submit() {
        let file = read_file();
        if file.items.iter().find(|r| r.id == old.id).is_some_and(|r| record_to_params(r) == params) {
            return Ok(old.id);
        }
        return Err("上一条成片提交结果尚未确认（可能仍在生成，或提交结果未知）；为防重复扣费，暂不提交新的成片任务".into());
    }
    let id = create_record(&params)?;
    stage_pending(id, request_id)?;
    spawn_generation(id, params, key);
    Ok(id)
}

/// 两段式重投：按原参数重新生成一条已存在的 running/failed 记录，同样立即返回。
/// `pending-verify` 的记录不许走这条——必须先经 `confirm_no_charge_and_resubmit` 显式确认。
pub fn resume_start(id: i64, request_id: Option<&str>, key: String) -> Result<i64, String> {
    if let Some(old) = pending_submit() {
        if old.id == id { return Ok(old.id); }
        return Err("上一条成片提交结果尚未确认（可能仍在生成，或提交结果未知）；为防重复扣费，暂不重新生成".into());
    }
    let params = restart_record(id)?;
    stage_pending(id, request_id)?;
    spawn_generation(id, params, key);
    Ok(id)
}

fn spawn_generation(id: i64, params: ReelParams, key: String) {
    std::thread::spawn(move || {
        if !try_begin_run(id) { return; }
        set_progress(id, "dialogue", "【1/5】准备对白与分镜…");
        let _ = run(id, &params, &key, &|phase, detail| set_progress(id, phase, detail));
        clear_pending_submit(id);
        end_run(id);
    });
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
        let mut f = HistoryFile { version: 1, items: (0..13).map(|n| ReelRecord { id:n, status:"done".into(), ts:n, ..Default::default() }).collect() };
        prune(&mut f); assert_eq!(f.items.len(), MAX_ITEMS);
        assert!(f.items.iter().all(|r| !r.kept));
    }
    /// 核心判据（Phase 2）：`kept=true` 的记录既不从历史里移除，也不删它的磁盘文件——
    /// 即使总数远超 `MAX_ITEMS`，即使它是最老的一条。
    #[test] fn prune_keeps_kept_items() {
        let _sb = crate::testsandbox::enter("reel-prune-kept", &[".uking"]);
        std::fs::create_dir_all(video_dir()).unwrap();
        let kept_name = "reel-0.mp4".to_string();
        std::fs::write(video_dir().join(&kept_name), b"fake mp4 bytes").unwrap();
        let mut items: Vec<ReelRecord> = (0..15).map(|n| ReelRecord { id: n, status: "done".into(), ts: n, ..Default::default() }).collect();
        // id=0 是最老的一条（ts 最小），标记保留并挂上刚写的文件。
        items[0].kept = true;
        items[0].file = Some(kept_name.clone());
        items[0].project_id = Some("reel-0".into());
        let mut f = HistoryFile { version: 1, items };
        prune(&mut f);
        // 非保留记录被裁到 MAX_ITEMS，保留的那条额外多出来，不占裁剪配额。
        assert_eq!(f.items.iter().filter(|r| !r.kept).count(), MAX_ITEMS);
        let kept = f.items.iter().find(|r| r.id == 0).expect("kept record must stay in history");
        assert!(kept.kept);
        assert_eq!(kept.file.as_deref(), Some(kept_name.as_str()));
        assert!(video_dir().join(&kept_name).is_file(), "kept file must survive prune on disk");
    }
    /// 迁移兼容：旧版 history.json（没有 kept/project_id/provider/phase/detail 字段）
    /// 必须能正常反序列化，且新字段落到安全默认值。
    #[test] fn legacy_history_without_new_fields_deserializes_with_safe_defaults() {
        let legacy = r#"{"version":1,"items":[{"id":1,"prompt":"旧记录","status":"done","file":"reel-1.mp4","ts":1}]}"#;
        let f: HistoryFile = serde_json::from_str(legacy).expect("legacy history.json must still parse");
        assert_eq!(f.items.len(), 1);
        let r = &f.items[0];
        assert!(!r.kept);
        assert!(r.project_id.is_none());
        assert!(r.provider.is_none());
        assert!(r.phase.is_none());
        assert!(r.detail.is_none());
        assert_eq!(r.file.as_deref(), Some("reel-1.mp4"));
    }
    #[test] fn keep_record_copies_file_into_project_assets_and_is_idempotent() {
        let _sb = crate::testsandbox::enter("reel-keep", &[".uking"]);
        let p = ReelParams { prompt: "保留测试".into(), ..Default::default() };
        let id = create_record(&p).unwrap();
        std::fs::create_dir_all(video_dir()).unwrap();
        let name = format!("reel-{id}.mp4");
        std::fs::write(video_dir().join(&name), b"fake mp4 bytes").unwrap();
        update(id, "done", Some(name.clone()), None, false, vec![]);
        keep_record(id).unwrap();
        let item = list_history().into_iter().find(|it| it.id == id).unwrap();
        assert!(item.kept);
        let project_id = item.project_id.clone().unwrap();
        assert!(project_export_dir(&project_id).join(&name).is_file());
        assert!(project_manifest_path(&project_id).is_file());
        // 幂等：再调一次不报错、不重复处理。
        keep_record(id).unwrap();
    }
    #[test] fn pending_verify_blocks_automatic_resubmit_and_needs_explicit_confirm() {
        let _sb = crate::testsandbox::enter("reel-pending-verify", &[".uking"]);
        let p = ReelParams { prompt: "待核实测试".into(), ..Default::default() };
        let id = create_record(&p).unwrap();
        stage_pending(id, Some("test-request-id")).unwrap();
        // 进程"死"在缝里：没有任何本地跑者持有这个 id。
        recover_dangling_pending();
        assert_eq!(list_history().into_iter().find(|it| it.id == id).unwrap().status, "pending-verify");
        // 常规重投入口必须拒绝 pending-verify（不是 running/failed）。
        assert!(restart_record(id).is_err());
        // 唯一允许的路径：客户显式确认没扣费。只断言状态转换本身（不牵动会真的
        // 起 gen-reel.mjs 子进程的 `confirm_no_charge_and_resubmit`，那部分需要
        // ffmpeg/node，属于真机验收范围，不是单测范围）。
        reopen_pending_verify_for_resubmit(id).unwrap();
        assert_eq!(list_history().into_iter().find(|it| it.id == id).unwrap().status, "running");
        assert!(pending_submit().is_none(), "confirming resubmit must clear the stale pending transaction");
    }
    #[test] fn one_local_runner_owns_each_reel_id() {
        let id = 5_551_212_i64;
        assert!(try_begin_run(id));
        assert!(!try_begin_run(id), "same reel id must not have two local generators");
        end_run(id);
        assert!(try_begin_run(id));
        end_run(id);
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
