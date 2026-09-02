//! AI 视频 —— 异步任务（提交 → 轮询 → 下载）。**独立可插拔模块**（删它只动 lib.rs + App.tsx）。
//!
//! 端点（2026-06-22 用真 Key 实测打通，veo-3.1-fast 出 9.8MB 合法 mp4）：
//!   - 提交  `POST /v1/video/generations`  `{model,prompt}` → `{task_id, status:"queued"}`
//!   - 轮询  `GET  /v1/video/generations/{task_id}` → `{code, data:{status,progress,fail_reason,result_url}}`
//!            status: `queued → IN_PROGRESS → SUCCESS | FAILURE`
//!   - 取片  `data.result_url`（`api.u-claw.org` → `api.u-claw.org.cn` 改写，裸网可达）+ auth → mp4 流
//!
//! 视频本体（~10MB）落 `~/.uking/video/<id>.mp4`，历史索引 `history.json` 只存元数据（仿 draw.rs）。
//! **历史列表不返回视频字节**（10MB×N 太重），前端点「播放」才按需 `read_video` 取磁盘路径，
//! 用 Tauri asset 协议直接流播（不再整档编 base64 塞 IPC，大文件会让 WebView2 卡住）。
//!
//! 模块铁律：只暴露纯函数 + 进度回调，`#[tauri::command]` 全在 lib.rs 转调（本模块不碰 AppHandle）。
//! HTTP 复用 `installer::curl`；自带极小 POST/GET 包装与 base64（叶子工具，保持自包含可插拔）。

use crate::installer::curl;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

const SUBMIT_URL: &str = "https://api.u-claw.org.cn/v1/video/generations";
const POLL_BASE: &str = "https://api.u-claw.org.cn/v1/video/generations/";

/// 轮询上限：5s/次 × 240 = 20 分钟（veo-fast 实测约 60s，给足慢模型/排队余量）。
const POLL_INTERVAL_S: u64 = 5;
const POLL_MAX: u32 = 240;
const MAX_ITEMS: usize = 12;

// App 启动恢复与视频页首次加载可能同时撞到同一条记录。业务任务只允许一个本地轮询/下载者，
// 否则两个线程会同时写同一个 mp4、互删临时失败文件，最后把已交付任务反而写成失败。
static ACTIVE_RUNS: OnceLock<Mutex<HashSet<i64>>> = OnceLock::new();

/// 一条视频记录（成功有 file，失败有 error，进行中 status=running 且 file/error 均空）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoRecord {
    pub id: i64,
    pub prompt: String,
    pub model: String,
    pub task_id: String,
    /// 成功：mp4 文件名（video/ 下的 <id>.mp4）
    #[serde(default)]
    pub file: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    /// running | ready（上游完成、待下载）| done | failed
    pub status: String,
    pub ts: i64,
}

/// 回前端的列表项（**不含视频字节**；have_video 标记是否可播放）。
#[derive(Debug, Clone, Serialize)]
pub struct VideoItemOut {
    pub id: i64,
    pub prompt: String,
    pub model: String,
    pub task_id: String,
    pub status: String,
    pub have_video: bool,
    pub error: Option<String>,
    pub ts: i64,
}

/// 轮询归一结果。
pub struct PollOut {
    /// running | done | failed
    pub phase: String,
    /// 形如 "50%"（直接透传给前端）
    pub progress: String,
    pub result_url: Option<String>,
    pub fail: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct HistoryFile {
    version: u32,
    items: Vec<VideoRecord>,
}

fn uking_home() -> PathBuf {
    let home = std::env::var("UKING_TEST_HOME")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| std::env::var("USERPROFILE").ok())
        .or_else(|| std::env::var("HOME").ok())
        .unwrap_or_else(|| ".".into());
    PathBuf::from(home).join(".uking")
}

/// GUI 提交前的写前日志。必须包含首帧图：如果进程死在 POST 已到服务端、task_id 响应未回本机
/// 的缝里，重启要用完全相同的 body + request_id 重放；服务端会返回原任务且不再扣费。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingSubmit {
    pub request_id: String,
    pub prompt: String,
    pub model: String,
    #[serde(default)]
    pub image: Option<String>,
    pub ts: i64,
}
fn video_dir() -> PathBuf {
    uking_home().join("video")
}
fn history_path() -> PathBuf {
    video_dir().join("history.json")
}
fn pending_submit_path() -> PathBuf {
    video_dir().join("pending-submit.json")
}
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn read_file() -> HistoryFile {
    std::fs::read_to_string(history_path())
        .ok()
        .and_then(|s| serde_json::from_str::<HistoryFile>(&s).ok())
        .unwrap_or(HistoryFile { version: 1, items: Vec::new() })
}
fn write_file(f: &HistoryFile) -> Result<(), String> {
    let _ = std::fs::create_dir_all(video_dir());
    let s = serde_json::to_string_pretty(f).map_err(|e| format!("序列化视频历史失败: {e}"))?;
    std::fs::write(history_path(), s).map_err(|e| format!("写入视频历史失败: {e}"))
}

/// 新建一条 running 记录（提交成功后立刻落盘，app 中途关也留痕，重开可续）。返回 id。
pub fn create_record(prompt: &str, model: &str, task_id: &str) -> Result<i64, String> {
    let _ = std::fs::create_dir_all(video_dir());
    let id = now_ms();
    let mut f = read_file();
    if let Some(old) = f.items.iter().find(|r| r.task_id == task_id) {
        return Ok(old.id);
    }
    f.items.insert(
        0,
        VideoRecord {
            id,
            prompt: prompt.to_string(),
            model: model.to_string(),
            task_id: task_id.to_string(),
            file: None,
            error: None,
            status: "running".into(),
            ts: id,
        },
    );
    prune(&mut f);
    f.version = 1;
    write_file(&f)?;
    Ok(id)
}

fn set_record_state(id: i64, status: &str, file: Option<String>, error: Option<String>) {
    let mut f = read_file();
    if let Some(r) = f.items.iter_mut().find(|r| r.id == id) {
        r.file = file;
        r.error = error;
        r.status = status.to_string();
    }
    let _ = write_file(&f);
}

pub fn stage_submit(prompt: &str, model: &str, image: Option<&str>) -> Result<PendingSubmit, String> {
    stage_submit_with_id(prompt, model, image, None)
}

/// 同一 `request_id` 的重放必须回到同一上游任务，不能再次扣费。GUI/CLI/MCP 的
/// ActionParity execution id 会通过这里落成视频提交幂等键；旧调用不传时仍生成本地键。
pub fn stage_submit_with_id(prompt: &str, model: &str, image: Option<&str>, request_id: Option<&str>) -> Result<PendingSubmit, String> {
    if let Some(old) = pending_submit() {
        if old.prompt == prompt && old.model == model && old.image.as_deref() == image {
            return Ok(old);
        }
        return Err("上一条视频的提交结果尚未确认，U-King 正在恢复原任务；为防重复扣费，暂不提交新视频".into());
    }
    let seed = format!("{}|{}|{}|{}", now_ms(), std::process::id(), model, prompt);
    let request_id = request_id.filter(|id| !id.trim().is_empty()).map(str::to_string)
        .unwrap_or_else(|| format!("ukv1-{}", &blake3::hash(seed.as_bytes()).to_hex()[..32]));
    if request_id.len() > 160 || !request_id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')) {
        return Err("invalid_input: request_id 只能包含 ASCII 字母、数字、-、_、.".into());
    }
    let pending = PendingSubmit {
        request_id,
        prompt: prompt.to_string(),
        model: model.to_string(),
        image: image.map(str::to_string),
        ts: now_ms(),
    };
    let _ = std::fs::create_dir_all(video_dir());
    let tmp = pending_submit_path().with_extension(format!("json.{}.tmp", std::process::id()));
    std::fs::write(&tmp, serde_json::to_vec_pretty(&pending).map_err(|e| e.to_string())?)
        .map_err(|e| format!("保存视频提交事务失败: {e}"))?;
    std::fs::rename(&tmp, pending_submit_path()).map_err(|e| format!("提交事务落盘失败: {e}"))?;
    Ok(pending)
}

pub fn pending_submit() -> Option<PendingSubmit> {
    std::fs::read_to_string(pending_submit_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
}

pub fn clear_pending_submit(request_id: &str) {
    if pending_submit()
        .as_ref()
        .is_some_and(|p| p.request_id == request_id)
    {
        let _ = std::fs::remove_file(pending_submit_path());
    }
}

pub fn submit_error_is_definitive(error: &str) -> bool {
    let low = error.to_lowercase();
    low.contains("余额不足")
        || low.contains("invalid token")
        || low.contains("unauthorized")
        || low.contains("缺少 api key")
        || low.contains("安全系统拒绝")
        || low.contains("参数")
        || low.contains("模型不存在")
        || low.contains("暂未接入")
}

/// 更新一条记录的最终态（done+file / failed+error）。
fn finalize_record(id: i64, file: Option<String>, error: Option<String>) {
    let status = if file.is_some() { "done" } else { "failed" };
    set_record_state(id, status, file, error);
}

fn mark_ready(id: i64, error: String) {
    set_record_state(id, "ready", None, Some(error));
}

pub fn try_begin_run(id: i64) -> bool {
    ACTIVE_RUNS
        .get_or_init(|| Mutex::new(HashSet::new()))
        .lock()
        .map(|mut ids| ids.insert(id))
        .unwrap_or(false)
}

pub fn end_run(id: i64) {
    if let Ok(mut ids) = ACTIVE_RUNS.get_or_init(|| Mutex::new(HashSet::new())).lock() {
        ids.remove(&id);
    }
}

pub fn recoverable_ids() -> Vec<i64> {
    read_file()
        .items
        .into_iter()
        .filter(|r| r.status == "running" || r.status == "ready")
        .map(|r| r.id)
        .collect()
}

/// 超上限的尾部记录连同 mp4 一起删（防 video/ 无限膨胀）。
fn prune(f: &mut HistoryFile) {
    while f.items.len() > MAX_ITEMS {
        if let Some(old) = f.items.pop() {
            if let Some(name) = old.file {
                let _ = std::fs::remove_file(video_dir().join(name));
            }
        }
    }
}

/// 历史列表（最近在前，不含视频字节）。
pub fn list_history() -> Vec<VideoItemOut> {
    read_file()
        .items
        .into_iter()
        .map(|r| VideoItemOut {
            id: r.id,
            prompt: r.prompt,
            model: r.model,
            task_id: r.task_id,
            have_video: r
                .file
                .as_ref()
                .map(|n| video_dir().join(n).exists())
                .unwrap_or(false),
            status: r.status,
            error: r.error,
            ts: r.ts,
        })
        .collect()
}

/// 清空视频历史（删 history.json + video/ 下所有 mp4）。
pub fn clear_history() -> Result<(), String> {
    for r in read_file().items {
        if let Some(name) = r.file {
            let _ = std::fs::remove_file(video_dir().join(name));
        }
    }
    let _ = std::fs::remove_file(history_path());
    Ok(())
}

// ── 任务流：提交 / 轮询 / 下载 ──────────────────────────────

/// 提交视频任务，返回 task_id。
/// `image`：可选首帧图（图生视频）。data URL / http url / 裸 base64 都接受，归一成 `image_url` 字段
/// （new-api/AI-ML schema 实测可达：veo-3.1 系图生视频）。无图 = 纯文生视频。
pub fn submit(
    api_key: &str,
    model: &str,
    prompt: &str,
    image: Option<&str>,
    request_id: Option<&str>,
) -> Result<String, String> {
    if api_key.trim().is_empty() {
        return Err("缺少 API Key（请先充值开通虾盘云）".into());
    }
    if prompt.trim().is_empty() {
        return Err("请先输入视频画面描述（图生视频也要写：希望首帧怎么动）".into());
    }
    let mut body = json!({ "model": model, "prompt": prompt });
    if let Some(img) = image.map(str::trim).filter(|s| !s.is_empty()) {
        // 已是 data URL / http(s) 直接用；裸 base64 补成 png data URL（前端 FileReader 给的就是完整 data URL）
        let image_url = if img.starts_with("http") || img.starts_with("data:") {
            img.to_string()
        } else {
            format!("data:image/png;base64,{img}")
        };
        body["image_url"] = json!(image_url);
    }
    if let Some(id) = request_id.map(str::trim).filter(|s| !s.is_empty()) {
        body["idempotency_key"] = json!(id);
    }
    // 图生视频(i2v) 的 body 带整张首帧图 base64（手机照片可达数 MB），即梦/图像中转 A 上游需
    // **同步**上传+处理图片才回 task_id —— 实测 2.27MB 图（3MB base64）≈114s。原写死 60s 远不够，
    // 客户 pc-*** Issue #57 实锤：图生视频 submit「curl 28 timed out 0 bytes received」。
    // 纯文生视频秒回；`-m` 是上限不是固定等待，故统一给足、有图再加大，不拖慢 t2v。
    let submit_timeout = if body.get("image_url").is_some() { 300 } else { 120 };
    // 提交留痕：视频是长任务且按次扣费，失败时客户只会说「没生成」。记模型 / 是否图生视频 /
    // 提示词前 60 字，**绝不记 api_key、不记 image_url**（那是整张图的 base64，几 MB）。
    let brief: String = prompt.chars().take(60).collect();
    crate::ulog::write(
        "video",
        &format!(
            "提交任务 model={model} 模式={} prompt={brief}",
            if image.is_some() { "图生视频" } else { "文生视频" }
        ),
    );
    let v = post_json(SUBMIT_URL, api_key, &body, submit_timeout).map_err(|e| {
        let cn = friendlier_video_error(e);
        crate::ulog::write("video", &format!("提交失败：{cn}"));
        cn
    })?;
    if let Some(e) = err_of(&v) {
        return Err(friendlier_video_error(e));
    }
    v.get("task_id")
        .or_else(|| v.get("id"))
        .and_then(|x| x.as_str())
        .map(String::from)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("提交响应缺少 task_id：{}", snippet(&v.to_string(), 200)))
}

/// 轮询一次任务状态。
pub fn poll(api_key: &str, task_id: &str) -> Result<PollOut, String> {
    let url = format!("{POLL_BASE}{task_id}");
    let v = get_json(&url, api_key, 30)?;
    if let Some(e) = err_of(&v) {
        return Err(e);
    }
    // 正常结构 { code:"success", data:{ status, progress, fail_reason, result_url } }
    let d = v.get("data").unwrap_or(&v);
    let raw = d.get("status").and_then(|x| x.as_str()).unwrap_or("").to_uppercase();
    let progress = d
        .get("progress")
        .and_then(|x| x.as_str())
        .map(String::from)
        .or_else(|| d.get("progress").and_then(|x| x.as_i64()).map(|n| format!("{n}%")))
        .unwrap_or_default();
    let result_url = d.get("result_url").and_then(|x| x.as_str()).map(String::from);
    let fail = d
        .get("fail_reason")
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from);

    let phase = if raw.contains("SUCCESS") || raw.contains("SUCCEED") || raw.contains("COMPLET") {
        "done"
    } else if raw.contains("FAIL") || raw.contains("ERROR") || raw.contains("CANCEL") {
        "failed"
    } else {
        "running"
    };
    Ok(PollOut {
        phase: phase.into(),
        progress,
        result_url,
        fail,
    })
}

/// 下载视频到目标路径。result_url 经 new-api 代理回 `api.u-claw.org/v1/videos/<task>/content`
/// （veo 上游真身在 easyrouter.io，要 ER 密钥；new-api 服务端注入后凭**客户 key** 鉴权代理），
/// 走 `to_cn` 改写到 .cn + auth。**自带 http/类型/魔数校验 + 重试 + 真因透传**：
///
/// 历史「视频下载失败（文件为空）」(#48/#54/#56) 真因 = 偶发拿到一段 <1KB 的 401/404 JSON
/// 错误体，旧逻辑只判 `len>1024` 就当失败、且把 opaque「文件为空」透出去 —— 客户没法定位、也不重试。
/// 现在：① 校验 http 2xx + 内容确是视频(类型或 mp4 魔数)；② 失败把错误体抠成人话；③ 重试 3 次
/// （上游 veo 偶发波动/瞬断，重试即好）。
pub fn download(api_key: &str, result_url: &str, dest: &PathBuf) -> Result<(), String> {
    let url = to_cn(result_url);
    let auth = format!("Authorization: Bearer {api_key}");
    let dest_s = dest.display().to_string();
    let _ = std::fs::create_dir_all(video_dir());
    let mut last = "视频下载失败，请稍后重试".to_string();
    for attempt in 1..=3 {
        let _ = std::fs::remove_file(dest);
        // body 落 dest；-w 把 http 码+类型打到 stdout（系统 curl 才有此格式，走 .NET 回退则不是）。
        // -L 跟跳转、--ssl-no-revoke 防 schannel CRYPT_E_REVOCATION_OFFLINE（与作图 ensure_b64 同坑）。
        let meta = curl(&[
            "-sS", "-m", "300", "-L", "--ssl-no-revoke", "-H", &auth,
            "-w", "UKHTTP=%{http_code};%{content_type}", "-o", &dest_s, &url,
        ]);
        match meta {
            Ok(info) if info.starts_with("UKHTTP=") => {
                let rest = &info["UKHTTP=".len()..];
                let (code, ctype) = rest.split_once(';').unwrap_or((rest, ""));
                let code = code.trim();
                if code.starts_with('2') && file_is_video(dest, ctype) {
                    return Ok(());
                }
                last = friendlier_video_error(dl_failure_reason(code, ctype, dest));
            }
            // 走了 .NET 回退（系统 curl TLS 失败）—— 该路径不写 dest，二进制拿不回，只能重试/透出
            Ok(_) => {
                last = "下载视频时网络/TLS 异常（已自动重试），多次失败请过一会儿再试或反馈".to_string();
            }
            Err(e) => last = friendlier_video_error(e),
        }
        if attempt < 3 {
            std::thread::sleep(std::time::Duration::from_secs(3));
        }
    }
    let _ = std::fs::remove_file(dest);
    Err(last)
}

/// 校验落盘的确是视频：体积够 +（content-type 是视频 或 mp4/mov 魔数 `ftyp`）。
/// 挡住「一段 >1KB 的 HTML/JSON 错误体被当 mp4 存下」这类假成功。
fn file_is_video(dest: &PathBuf, ctype: &str) -> bool {
    let len = std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0);
    if len <= 4096 {
        return false;
    }
    let lc = ctype.to_lowercase();
    if lc.contains("video/") || lc.contains("mp4") || lc.contains("octet-stream") {
        return true;
    }
    // 类型不可信时看魔数：mp4/mov 头部第 4 字节起是 "ftyp"
    use std::io::Read;
    let mut buf = [0u8; 12];
    std::fs::File::open(dest)
        .and_then(|mut f| f.read_exact(&mut buf))
        .map(|_| &buf[4..8] == b"ftyp")
        .unwrap_or(false)
}

/// 下载失败时把 dest 里的（小）错误体抠成可读真因，取代 opaque「文件为空」。
fn dl_failure_reason(code: &str, ctype: &str, dest: &PathBuf) -> String {
    let body = std::fs::read_to_string(dest).unwrap_or_default();
    // 服务端常回 {"error":{"message":"..."}}
    if let Ok(v) = serde_json::from_str::<Value>(&body) {
        if let Some(m) = v
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|x| x.as_str())
        {
            return format!("下载失败(http={code})：{m}");
        }
    }
    let snip = snippet(body.trim(), 160);
    if snip.is_empty() {
        format!("下载失败(http={code}, 类型={ctype})：服务器没有返回视频内容，请稍后重试")
    } else {
        format!("下载失败(http={code})：{snip}")
    }
}

/// 把视频提交/出片/下载的常见报错翻成人话（余额不足 / 超时 / 令牌问题）。仿 `friendlier_image_error`。
pub fn friendlier_video_error(e: String) -> String {
    let low = e.to_lowercase();
    // 余额不足（submit 预扣失败）：AI 视频成本较高，解析出本条需要 / 当前余额，引导充值
    if low.contains("quota is not enough") || e.contains("余额不足") || low.contains("insufficient") {
        let mut s = String::from("余额不足：AI 视频成本较高");
        if let Some(n) = extract_cny(&e, "need quota") {
            s.push_str(&format!("，本条约需 ¥{n}"));
        }
        if let Some(r) = extract_cny(&e, "remain quota") {
            s.push_str(&format!("，当前余额 ¥{r}"));
        }
        s.push_str("。请点右上角「充值」补足后再试。");
        return s;
    }
    if low.contains("invalid token") || low.contains("unauthorized") || low.contains("401") {
        return "Key 暂时不可用：通常是这台电脑的虾盘云 Key 还没有充值开通，或余额已用完。请点右上角「充值」后再试。".to_string();
    }
    if low.contains("safety system") || low.contains("policy") || low.contains("content filter") {
        return "视频安全系统拒绝了这个提示词。通常是因为包含真实人物姓名、具体影视/游戏 IP、敏感肖像或容易被误判的表达。请换个更中性的描述再试。".to_string();
    }
    if e.contains("(28)") || low.contains("timed out") || low.contains("timeout") || e.contains("超时") {
        return "视频生成/下载超时：可能这条较复杂或当前网络较慢，请过一两分钟再重试一次。".to_string();
    }
    // 上游偶发（内容审核 / 波动），veo 实测重试即好
    if low.contains("task failed") || e.contains("生成失败") {
        return "视频生成失败（上游偶发，多为内容审核或波动）：换个描述、或稍后点「重试」通常即好。".to_string();
    }
    e
}

/// 从形如 "need quota: ¥46.72" 抠出数字串。
fn extract_cny(s: &str, label: &str) -> Option<String> {
    let i = s.find(label)?;
    let rest = &s[i + label.len()..];
    let digits: String = rest
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    if digits.is_empty() {
        return None;
    }
    // 去掉尾零让文案干净：46.720000 → 46.72，10.000000 → 10
    let trimmed = if digits.contains('.') {
        digits.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        digits
    };
    Some(trimmed)
}

/// 跑完一条已存在记录的任务：轮询到终态 → 成功则下载落盘 + finalize。进度走 `on_progress(phase, progress)`。
/// 纯函数 + 回调，lib.rs 负责 spawn_blocking + emit。
pub fn run(
    api_key: &str,
    id: i64,
    task_id: &str,
    on_progress: &dyn Fn(&str, &str),
) -> Result<(), String> {
    for _ in 0..POLL_MAX {
        match poll(api_key, task_id) {
            Ok(p) => {
                on_progress(&p.phase, &p.progress);
                match p.phase.as_str() {
                    "done" => {
                        let Some(url) = p.result_url else {
                            let e = "服务端已生成视频，但暂时没有返回下载地址；任务已保留，稍后会自动重试下载".to_string();
                            mark_ready(id, e.clone());
                            return Err(e);
                        };
                        let name = format!("{id}.mp4");
                        let dest = video_dir().join(&name);
                        if let Err(e) = download(api_key, &url, &dest) {
                            // 「上游出片了但本机没落盘」和「上游根本没出片」是完全不同的故障，
                            // 分开记，否则远程只看到一句「没生成」没法区分。
                            crate::ulog::write("video", &format!("task={task_id} 下载失败：{e}"));
                            // 上游已经出片，绝不能写成 failed 后诱导客户再点一次生成、再扣一次钱。
                            // 保留 ready + task_id，启动恢复/“重试下载”只查原任务、绝不 POST。
                            mark_ready(id, format!("视频已生成但尚未下载：{e}"));
                            return Err(e);
                        }
                        crate::ulog::write("video", &format!("task={task_id} 出片成功 → {name}"));
                        finalize_record(id, Some(name), None);
                        return Ok(());
                    }
                    "failed" => {
                        let e = friendlier_video_error(
                            p.fail.unwrap_or_else(|| "视频生成失败".into()),
                        );
                        crate::ulog::write("video", &format!("task={task_id} 上游判失败：{e}"));
                        finalize_record(id, None, Some(e.clone()));
                        return Err(e);
                    }
                    _ => {}
                }
            }
            // 单次轮询网络抖动不致命，继续重试
            Err(_) => {}
        }
        std::thread::sleep(std::time::Duration::from_secs(POLL_INTERVAL_S));
    }
    let e = "视频任务 20 分钟仍未返回终态；原任务已保留，稍后会继续查询，不会重新生成或扣费".to_string();
    crate::ulog::write("video", &format!("task={task_id} 轮询超时（{POLL_MAX} 次未出结果）"));
    set_record_state(id, "running", None, Some(e.clone()));
    Err(e)
}

/// 某条视频记录的磁盘 mp4 路径（导出/另存为用）。不存在返回 None。
pub fn video_file_path(id: i64) -> Option<PathBuf> {
    read_file()
        .items
        .into_iter()
        .find(|r| r.id == id)
        .and_then(|r| r.file)
        .map(|name| video_dir().join(name))
        .filter(|p| p.exists())
}

/// 取某条记录的 task_id（resume 用）。
pub fn task_id_of(id: i64) -> Option<String> {
    read_file().items.into_iter().find(|r| r.id == id).map(|r| r.task_id)
}

/// 把本地图片文件读成 `image_url` 要的 data URL（`--video-test` 验图生视频用；前端走 FileReader 不需要它）。
pub fn image_file_to_data_url(path: &str) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("读图片失败: {e}"))?;
    let mime = if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "image/jpeg"
    } else {
        "image/png"
    };
    Ok(format!("data:{mime};base64,{}", b64_encode(&bytes)))
}

// ── HTTP / 工具 ────────────────────────────────────────────

/// POST JSON（body 落临时文件，绕命令行引号/中文）。复用 installer::curl。
fn post_json(url: &str, api_key: &str, body: &Value, timeout_s: u32) -> Result<Value, String> {
    let tmp = std::env::temp_dir().join(format!("uking-vreq-{}.json", std::process::id()));
    std::fs::write(&tmp, serde_json::to_vec(body).map_err(|e| e.to_string())?)
        .map_err(|e| format!("写请求临时文件失败: {e}"))?;
    let data = format!("@{}", tmp.display());
    let tmo = timeout_s.to_string();
    let auth = format!("Authorization: Bearer {api_key}");
    let out = curl(&[
        "-sS", "-m", &tmo, "-X", "POST", url, "-H", &auth, "-H",
        "Content-Type: application/json", "--data", &data,
    ]);
    let _ = std::fs::remove_file(&tmp);
    let out = out?;
    serde_json::from_str(&out).map_err(|_| format!("响应不是 JSON：{}", snippet(&out, 300)))
}

fn get_json(url: &str, api_key: &str, timeout_s: u32) -> Result<Value, String> {
    let tmo = timeout_s.to_string();
    let auth = format!("Authorization: Bearer {api_key}");
    let out = curl(&["-sS", "-m", &tmo, "-H", &auth, url])?;
    serde_json::from_str(&out).map_err(|_| format!("响应不是 JSON：{}", snippet(&out, 300)))
}

/// 从响应里抠出错误文案（兼容 {error:{message}} 与 {code,message}）。
fn err_of(v: &Value) -> Option<String> {
    if let Some(e) = v.get("error").filter(|e| !e.is_null()) {
        return Some(
            e.get("message")
                .and_then(|x| x.as_str())
                .unwrap_or(&e.to_string())
                .to_string(),
        );
    }
    // {code:"xxx_failed", message:"..."}（成功是 code:"success"）
    if let Some(code) = v.get("code").and_then(|x| x.as_str()) {
        if code != "success" && !code.is_empty() {
            let msg = v.get("message").and_then(|x| x.as_str()).unwrap_or(code);
            if !msg.is_empty() {
                return Some(msg.to_string());
            }
        }
    }
    None
}

/// api.u-claw.org → api.u-claw.org.cn（裸网可达）。只换裸 .org 主机，已是 .org.cn 的不动。
fn to_cn(u: &str) -> String {
    u.replace("://api.u-claw.org/", "://api.u-claw.org.cn/")
}

fn snippet(s: &str, n: usize) -> String {
    let t = s.trim();
    if t.chars().count() <= n {
        t.to_string()
    } else {
        let cut: String = t.chars().take(n).collect();
        format!("{cut}…")
    }
}

// ── base64 编码（self-contained，保持模块可插拔）──
const B64_CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
fn b64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | (b[2] as u32);
        out.push(B64_CHARS[((n >> 18) & 63) as usize] as char);
        out.push(B64_CHARS[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 { B64_CHARS[((n >> 6) & 63) as usize] as char } else { '=' });
        out.push(if chunk.len() > 2 { B64_CHARS[(n & 63) as usize] as char } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ready_video_stays_recoverable_instead_of_becoming_failed() {
        let _sb = crate::testsandbox::enter("video-ready", &[".uking"]);
        let id = create_record("测试镜头", "model", "task-ready").expect("create record");
        mark_ready(id, "视频已生成但下载中断".into());

        let item = list_history().into_iter().find(|v| v.id == id).expect("history item");
        assert_eq!(item.status, "ready");
        assert!(!item.have_video);
        assert!(item.error.unwrap_or_default().contains("下载中断"));
        assert_eq!(recoverable_ids(), vec![id]);
        assert_eq!(task_id_of(id).as_deref(), Some("task-ready"));
    }

    #[test]
    fn one_local_runner_owns_each_paid_task() {
        let id = 9_876_543_210_i64;
        assert!(try_begin_run(id));
        assert!(!try_begin_run(id), "same paid task must not have two local downloaders");
        end_run(id);
        assert!(try_begin_run(id));
        end_run(id);
    }

    #[test]
    fn submit_transaction_survives_response_gap_and_task_record_is_idempotent() {
        let _sb = crate::testsandbox::enter("video-submit-tx", &[".uking"]);
        let pending = stage_submit("镜头二", "model-fast", Some("data:image/png;base64,AA=="))
            .expect("stage submit");
        let loaded = pending_submit().expect("pending submit persisted");
        assert_eq!(loaded.request_id, pending.request_id);
        assert_eq!(loaded.image, pending.image);

        let first = create_record("镜头二", "model-fast", "task-same").expect("first record");
        let replay = create_record("镜头二", "model-fast", "task-same").expect("idempotent replay");
        assert_eq!(first, replay, "same server task must not create duplicate local history");
        clear_pending_submit(&pending.request_id);
        assert!(pending_submit().is_none());
    }

    #[test]
    fn action_execution_id_becomes_the_paid_request_id() {
        let _sb = crate::testsandbox::enter("video-action-execution-id", &[".uking"]);
        let pending = stage_submit_with_id(
            "镜头三",
            "model-fast",
            None,
            Some("uking-18f2-a-7"),
        )
        .expect("stage action request");

        assert_eq!(pending.request_id, "uking-18f2-a-7");
        assert_eq!(pending_submit().as_ref().map(|p| p.request_id.as_str()), Some("uking-18f2-a-7"));
    }

    #[test]
    fn unsafe_execution_id_is_rejected_before_a_paid_request_is_staged() {
        let _sb = crate::testsandbox::enter("video-invalid-execution-id", &[".uking"]);
        let error = stage_submit_with_id("镜头四", "model-fast", None, Some("bad key with space"))
            .expect_err("unsafe id must not reach a provider");

        assert!(error.starts_with("invalid_input:"));
        assert!(pending_submit().is_none());
    }
}
