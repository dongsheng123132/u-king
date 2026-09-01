//! 工作台聊天历史的**文件化存档** —— `~/.uking/chats/<sessionId>.jsonl`。
//!
//! ## 它治什么病（2026-08-25，fable5 架构评审 + 客户机实锤）
//!
//! 聊天历史原来只存在 WebView 的 localStorage 里，有三个结构性病：
//! 1. **配额满静默丢历史**：WebView localStorage 只有几 MB，「保存失败就放弃」意味着
//!    聊得越多的人越容易丢——丢的还恰恰是最该留的长对话；
//! 2. **孤儿档永不回收**：「新建对话」（工具型会话）不落 tasks.json，但它的 localStorage
//!    存档照写。会话一关，档案变成没有任何 UI 能再摸到的孤儿；
//! 3. **删除要反向扫全库**：关会话时得遍历 localStorage 找前缀匹配的 key 才能清干净，
//!    元数据（tasks.json）与消息（localStorage）劈在两个互不知情的存储层。
//!
//! 消息是 append-heavy 的顺序写 → **JSONL（一行一条）**：追加 = 打开+写尾，不用整档重写；
//! 读侧整档载入后过滤。裁剪规则沿用原 localStorage 版（去 approval、去图片 b64、截超长输出），
//! 由前端负责——本模块只管字节进出，不懂消息语义。
//!
//! ## 纪律（照抄 threads.rs / device.rs 的家规）
//! - 写盘原子性靠「append 单行」天然近似原子（单行 <4KB 时 POSIX append 语义足够）；
//!   全量重写（compact）走临时文件 + rename，绝不半截覆盖。
//! - sessionId 只允许 `[A-Za-z0-9._-]`，防路径穿越——它直接拼进文件名。
//! - 纯 std + serde_json，零新依赖。

use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;

/// 单个会话存档的硬上限（字节）。超过后拒绝追加并提示压缩 —— 防止单会话把盘写穿。
/// 400KB ≈ 一部中长篇小说，正常对话一辈子摸不到；真摸到说明该开新会话了。
const MAX_ARCHIVE_BYTES: u64 = 400 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    /// 前端 Item 的自由结构（text/tool/approval…），这里不解释语义只存字节
    #[serde(flatten)]
    pub data: serde_json::Value,
}

fn uking_home() -> PathBuf {
    // 认 `UKING_TEST_HOME` 沙箱（与 aitasks/automation 同口径）——测试写真实家目录是宪法 10。
    if let Ok(t) = std::env::var("UKING_TEST_HOME") {
        if !t.is_empty() {
            return PathBuf::from(t);
        }
    }
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".uking")
}

fn chats_dir() -> PathBuf {
    uking_home().join("chats")
}

/// sessionId → 文件名。非法字符一律拒绝（返回 None），调用方转错误。
///
/// 允许集 `[A-Za-z0-9._-]` 正好覆盖三种现有 id 形状：
/// `sess-<folderhash>-<n>` / `sess-tool-<tool>-<n>` / `native-chat`。
fn archive_path(session_id: &str) -> Option<PathBuf> {
    if session_id.is_empty() || session_id.len() > 120 {
        return None;
    }
    if !session_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        return None;
    }
    if session_id.starts_with('.') {
        return None; // 不给隐藏/相对路径花样留门
    }
    Some(chats_dir().join(format!("{session_id}.jsonl")))
}

/// 追加一批消息到会话存档。前端每轮对话结束调一次（不是每条 delta 都调）。
#[tauri::command]
pub fn chat_archive_append(session_id: String, items: Vec<serde_json::Value>) -> Result<(), String> {
    if items.is_empty() {
        return Ok(());
    }
    let path = archive_path(&session_id).ok_or_else(|| format!("非法会话 id: {session_id}"))?;
    let _ = std::fs::create_dir_all(chats_dir());
    // 上限检查：追加前看一眼现有大小，超限拒绝并明说，让前端有机会提示「开新会话」。
    if let Ok(meta) = std::fs::metadata(&path) {
        if meta.len() > MAX_ARCHIVE_BYTES {
            return Err("这个会话的存档已到大小上限，请新建一个会话继续".into());
        }
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("打开聊天存档失败: {e}"))?;
    for it in &items {
        let line =
            serde_json::to_string(it).map_err(|e| format!("序列化消息失败: {e}"))?;
        f.write_all(line.as_bytes())
            .and_then(|_| f.write_all(b"\n"))
            .map_err(|e| format!("写入聊天存档失败: {e}"))?;
    }
    Ok(())
}

/// 读回整个会话的历史。文件不存在 = 空历史（新会话的正常形状，不报错）。
#[tauri::command]
pub fn chat_archive_load(session_id: String) -> Result<Vec<serde_json::Value>, String> {
    let path = archive_path(&session_id).ok_or_else(|| format!("非法会话 id: {session_id}"))?;
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return Ok(Vec::new()),
    };
    let mut out = Vec::new();
    for line in raw.lines() {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<serde_json::Value>(line) {
            Ok(v) => out.push(v),
            Err(_) => continue, // 单行坏了跳过，别让一条坏行废掉整个会话
        }
    }
    Ok(out)
}

/// 用一份完整列表**整体替换**会话存档（临时文件 + rename，绝不半截覆盖）。
/// 前端在「裁剪/去重」后调它；也用于从 localStorage 迁移旧数据。
#[tauri::command]
pub fn chat_archive_replace(session_id: String, items: Vec<serde_json::Value>) -> Result<(), String> {
    let path = archive_path(&session_id).ok_or_else(|| format!("非法会话 id: {session_id}"))?;
    let _ = std::fs::create_dir_all(chats_dir());
    let tmp = chats_dir().join(format!(
        ".{session_id}.tmp-{}",
        std::process::id()
    ));
    {
        let mut f = std::fs::File::create(&tmp).map_err(|e| format!("创建临时文件失败: {e}"))?;
        for it in &items {
            let line =
                serde_json::to_string(it).map_err(|e| format!("序列化消息失败: {e}"))?;
            f.write_all(line.as_bytes())
                .and_then(|_| f.write_all(b"\n"))
                .map_err(|e| format!("写入聊天存档失败: {e}"))?;
        }
    }
    std::fs::rename(&tmp, &path).map_err(|e| format!("替换聊天存档失败: {e}"))
}

/// 现存全部会话存档的 id 清单（不含扩展名）。侧栏 Standby 灯用：文件在 = 聊过。
///
/// 🔴 0 字节文件**不算**：挂载即水合的会话可能留下空档（前端已保证空列表不落盘，
/// 这里是第二道闸），「建了没说话」绝不能亮 Standby（假灯会诱导客户重开会话）。
#[tauri::command]
pub fn chat_archive_list() -> Vec<String> {
    let dir = chats_dir();
    let rd = match std::fs::read_dir(&dir) {
        Ok(rd) => rd,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for entry in rd.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if let Some(id) = name.strip_suffix(".jsonl") {
            if id.starts_with('.') {
                continue; // 临时文件
            }
            // 空文件跳过：等于「没聊过」
            match entry.metadata() {
                Ok(m) if m.len() > 0 => out.push(id.to_string()),
                _ => {}
            }
        }
    }
    out.sort();
    out
}

/// 删除会话存档。不存在算成功（幂等——删除路径要的就是「再来一次也不报错」）。
#[tauri::command]
pub fn chat_archive_delete(session_id: String) -> Result<(), String> {
    let path = archive_path(&session_id).ok_or_else(|| format!("非法会话 id: {session_id}"))?;
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("删除聊天存档失败: {e}")),
    }
}

// ───────────────────── 归档区（抄 claude-code-history-viewer 的经验）─────────────────────
//
// 「关闭会话」原来是二选一：留着 or 彻底删。客户的历史聊天记录没有中间态——
// 想留又不想占侧栏的，只能一直挂着。归档层补上这个缺口：
//   archived/<sid>.jsonl  = 原始消息文件**挪**进来（不是复制，不占双份空间）
//   archived/manifest.json = 全局清单（id/名字/条数/大小/时间），浏览零成本
// 恢复 = 挪回 chats/。彻底删除只在归档区提供。

fn archived_dir() -> PathBuf {
    chats_dir().join("archived")
}

/// 当前毫秒时间戳（照抄 tasks.rs 的实现，文件不存在等异常时返回 0）。
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 归档清单落盘结构。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchivedSessionInfo {
    pub session_id: String,
    /// 归档时的会话名（前端传，列表直接显示，不用再猜）
    pub name: String,
    /// 归档时会话绑定的项目文件夹（恢复时按它重建任务）。旧清单没有此字段 → None，
    /// 恢复时只挪文件不重建卡片。
    #[serde(default)]
    pub dir: Option<String>,
    pub message_count: u64,
    pub size_bytes: u64,
    pub archived_at_ms: i64,
}

#[derive(Debug, Serialize, Deserialize)]
struct ArchivedManifest {
    version: u32,
    sessions: Vec<ArchivedSessionInfo>,
}

impl Default for ArchivedManifest {
    fn default() -> Self {
        Self { version: 1, sessions: Vec::new() }
    }
}

/// 归档区全部写操作共用一把进程内锁：manifest 是「读→改→写」，并发归档两个会话
/// 会互相覆盖清单丢条目。四个命令各自拿锁、helpers 不拿（避免重入死锁）。
static ARCHIVE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn archive_guard() -> std::sync::MutexGuard<'static, ()> {
    // 中毒的锁说明有 panic 过，但守卫数据本身无不变量可破坏 —— 拿回继续用。
    match ARCHIVE_LOCK.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    }
}

fn archived_manifest_path() -> PathBuf {
    archived_dir().join("manifest.json")
}

fn read_archived_manifest() -> ArchivedManifest {
    std::fs::read_to_string(archived_manifest_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_archived_manifest(m: &ArchivedManifest) -> Result<(), String> {
    let _ = std::fs::create_dir_all(archived_dir());
    let path = archived_manifest_path();
    // 唯一临时名（PID + 毫秒）：同进程多线程并发写也不会互相踩
    let tmp = archived_dir().join(format!(".manifest.tmp-{}-{}", std::process::id(), now_ms()));
    let s = serde_json::to_string_pretty(m).map_err(|e| format!("序列化归档清单失败: {e}"))?;
    std::fs::write(&tmp, s).map_err(|e| format!("写归档清单失败: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("替换归档清单失败: {e}"))
}

fn archived_file_path(session_id: &str) -> Option<PathBuf> {
    // 复用同一套 id 白名单校验（archive_path 只拼目录不同）
    let p = archive_path(session_id)?;
    let name = p.file_name()?;
    Some(archived_dir().join(name))
}

/// 把一个会话挪进归档区。原文件不存在 = 没聊过，报错让前端别白归档。
#[tauri::command]
pub fn chat_session_archive(
    session_id: String,
    name: String,
    dir: Option<String>,
) -> Result<ArchivedSessionInfo, String> {
    let _g = archive_guard();
    let src = archive_path(&session_id).ok_or_else(|| format!("非法会话 id: {session_id}"))?;
    if !src.exists() {
        // 活跃区没文件只有两种可能：真没聊过（拒绝白归档），或**已经归档过**
        // （重复点关闭）。后者必须幂等成功——报错会把前端整个关会话流程打断。
        let dst = archived_file_path(&session_id)
            .ok_or_else(|| format!("非法会话 id: {session_id}"))?;
        if dst.exists() {
            let mut m = read_archived_manifest();
            if let Some(e) = m.sessions.iter_mut().find(|s| s.session_id == session_id) {
                if !name.trim().is_empty() {
                    e.name = name.trim().to_string();
                }
                let info = e.clone();
                write_archived_manifest(&m)?;
                return Ok(info);
            }
        }
        return Err("这个会话还没有聊天记录，不用归档".into());
    }
    let dst = archived_file_path(&session_id)
        .ok_or_else(|| format!("非法会话 id: {session_id}"))?;
    let _ = std::fs::create_dir_all(archived_dir());
    // 目标已存在（重复归档）→ 先清掉旧的，以本次为准
    let _ = std::fs::remove_file(&dst);
    std::fs::rename(&src, &dst).map_err(|e| format!("挪进归档区失败: {e}"))?;

    let size_bytes = std::fs::metadata(&dst).map(|m| m.len()).unwrap_or(0);
    // 条数 = 行数（JSONL 一行一条）；读一遍统计，几百 KB 内可忽略
    let message_count = std::fs::read_to_string(&dst)
        .map(|s| s.lines().filter(|l| !l.trim().is_empty()).count() as u64)
        .unwrap_or(0);

    let info = ArchivedSessionInfo {
        session_id,
        name: name.trim().to_string(),
        dir: dir.filter(|d| !d.trim().is_empty()),
        message_count,
        size_bytes,
        archived_at_ms: now_ms(),
    };
    let mut m = read_archived_manifest();
    m.sessions.retain(|s| s.session_id != info.session_id);
    m.sessions.insert(0, info.clone()); // 新归档排最前
    if let Err(e) = write_archived_manifest(&m) {
        // 清单写失败 = 这条归档 UI 看不见（孤儿）。把文件挪回活跃区，保持两边一致，
        // 让前端拿到明确的错误而不是静默丢记录。
        let _ = std::fs::rename(&dst, &src);
        return Err(e);
    }
    Ok(info)
}

/// 从归档区恢复：挪回活跃区。活跃区已有同名档（恢复前又开过同名会话）时**显式报错**，
/// 绝不静默覆盖——Windows 的 rename 不跨已存在目标，覆盖语义也不该藏在系统调用里。
#[tauri::command]
pub fn chat_session_restore(session_id: String) -> Result<(), String> {
    let _g = archive_guard();
    let src = archived_file_path(&session_id)
        .ok_or_else(|| format!("非法会话 id: {session_id}"))?;
    if !src.exists() {
        return Err("归档区里没有这个会话".into());
    }
    let dst = archive_path(&session_id).ok_or_else(|| format!("非法会话 id: {session_id}"))?;
    if dst.exists() {
        return Err(
            "活跃区已经有一个同名会话的聊天记录了。先把它归档或删除，再来恢复这份。".into(),
        );
    }
    let _ = std::fs::create_dir_all(chats_dir());
    std::fs::rename(&src, &dst).map_err(|e| format!("从归档区恢复失败: {e}"))?;

    let mut m = read_archived_manifest();
    m.sessions.retain(|s| s.session_id != session_id);
    if let Err(e) = write_archived_manifest(&m) {
        // 清单写失败同理回滚：文件挪回归档区，清单里那条还在，UI 与盘一致。
        let _ = std::fs::rename(&dst, &src);
        return Err(e);
    }
    Ok(())
}

/// 归档区清单（按归档时间倒序）。
#[tauri::command]
pub fn chat_archived_list() -> Vec<ArchivedSessionInfo> {
    let _g = archive_guard();
    let mut m = read_archived_manifest();
    // 清单与磁盘对账：文件被手动删掉的条目剔除（盘是真相，清单是缓存）。
    // 对账有变化就写回，否则下次任何归档写入又会把陈旧记录带回盘。
    let before = m.sessions.len();
    m.sessions.retain(|s| {
        archived_file_path(&s.session_id)
            .map(|p| p.exists())
            .unwrap_or(false)
    });
    if m.sessions.len() != before {
        let _ = write_archived_manifest(&m); // 只读命令尽力而为，失败留给下次对账
    }
    m.sessions.sort_by(|a, b| b.archived_at_ms.cmp(&a.archived_at_ms));
    m.sessions
}

/// 从归档区彻底删除（唯一的「找不回来」入口，前端必须二次确认后才调）。
#[tauri::command]
pub fn chat_session_purge(session_id: String) -> Result<(), String> {
    let _g = archive_guard();
    let path = archived_file_path(&session_id)
        .ok_or_else(|| format!("非法会话 id: {session_id}"))?;
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("删除归档失败: {e}")),
    }
    let mut m = read_archived_manifest();
    m.sessions.retain(|s| s.session_id != session_id);
    write_archived_manifest(&m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_path_rejects_traversal_and_weird_ids() {
        assert!(archive_path("sess-abc123-1").is_some());
        assert!(archive_path("native-chat").is_some());
        assert!(archive_path("sess-tool-claude-2").is_some());
        assert!(archive_path("").is_none());
        assert!(archive_path("../evil").is_none()); // 斜杠不在白名单
        assert!(archive_path("..").is_none());
        assert!(archive_path(".hidden").is_none()); // 点开头
        assert!(archive_path("有中文").is_none()); // 非 ASCII
        assert!(archive_path("a\\b").is_none()); // 反斜杠
        assert!(archive_path(&"x".repeat(121)).is_none()); // 超长
    }

    #[test]
    fn roundtrip_replace_load_delete() {
        // 🔴 必须进全进程唯一沙箱（crate::testsandbox）：本用例真写盘（chats/<sid>.jsonl），
        // 裸跑 = 写真实 ~/.uking（宪法 10）。2026-08-26 实证：没进沙箱时它和真实会话数据
        // 相撞，单独跑绿、全量并行红，且污染的是客户形态的真实目录。
        let _sb = crate::testsandbox::enter("chatstore-roundtrip", &[".uking"]);
        let sid = "sess-test-roundtrip";
        let items = vec![
            serde_json::json!({"type":"text","role":"user","content":"你好"}),
            serde_json::json!({"type":"text","role":"assistant","content":"你好！"}),
            serde_json::json!({"type":"tool","name":"run_command","output":"ok"}),
        ];
        chat_archive_replace(sid.to_string(), items.clone()).expect("replace");
        let loaded = chat_archive_load(sid.to_string()).expect("load");
        assert_eq!(loaded.len(), 3);
        assert_eq!(loaded[0]["content"], "你好");
        chat_archive_append(sid.to_string(), vec![serde_json::json!({"type":"text","role":"user","content":"再来一条"})])
            .expect("append");
        let loaded2 = chat_archive_load(sid.to_string()).expect("load2");
        assert_eq!(loaded2.len(), 4);
        chat_archive_delete(sid.to_string()).expect("delete");
        // 幂等：再删一次也不报错
        chat_archive_delete(sid.to_string()).expect("delete again");
        // 删完读回空
        assert!(chat_archive_load(sid.to_string()).expect("load empty").is_empty());
    }

    /// 归档全流程：写入活跃区 → 归档（挪走）→ 清单在 → 恢复（挪回）→ 彻底删除。
    /// 跑在 UKING_TEST_HOME 沙箱里（宪法 10），不碰真实 ~/.uking。
    #[test]
    fn archive_restore_purge_flow() {
        // 🔴 同上：真写盘（归档挪文件 + 改 manifest），必须持全进程唯一沙箱锁，
        // 否则与并行用例互踩（2026-08-26 全量跑两次稳定红的根因）。
        let _sb = crate::testsandbox::enter("chatstore-archive-flow", &[".uking"]);
        let sid = "sess-test-archive-flow";
        // ① 活跃区建档
        chat_archive_replace(
            sid.to_string(),
            vec![
                serde_json::json!({"type":"text","role":"user","content":"问题"}),
                serde_json::json!({"type":"text","role":"assistant","content":"回答"}),
            ],
        )
        .expect("setup");
        assert!(chat_archive_load(sid.to_string()).expect("load").len() == 2);

        // ② 归档：活跃区文件挪走，清单记上一笔
        let info =
            chat_session_archive(sid.to_string(), "测试会话".into(), Some("C:\\proj\\demo".into()))
                .expect("archive");
        assert_eq!(info.message_count, 2);
        assert_eq!(info.name, "测试会话");
        assert_eq!(info.dir.as_deref(), Some("C:\\proj\\demo"));
        assert!(chat_archive_load(sid.to_string()).expect("active gone").is_empty());
        let listed = chat_archived_list();
        assert!(listed.iter().any(|s| s.session_id == sid), "归档清单应包含该会话");

        // ②b 重复归档（活跃区已没文件）必须幂等成功——前端关会话流程不能被打断
        let again = chat_session_archive(sid.to_string(), "".into(), None).expect("archive twice");
        assert_eq!(again.message_count, 2);
        assert_eq!(again.name, "测试会话"); // 空名字不覆盖原记录

        // ③ 恢复：挪回活跃区，清单移除
        chat_session_restore(sid.to_string()).expect("restore");
        assert!(chat_archived_list().iter().all(|s| s.session_id != sid));
        assert!(chat_archive_load(sid.to_string()).expect("back").len() == 2);

        // ④ 再归档一次；然后制造「活跃区与归档区并存」的冲突场景
        chat_session_archive(sid.to_string(), "再归档".into(), None).expect("archive2");
        chat_archive_replace(
            sid.to_string(),
            vec![serde_json::json!({"type":"text","role":"user","content":"同 id 又开了新会话说的话"})],
        )
        .expect("recreate active");
        // ⑤ 冲突保护：活跃区已有同名档，恢复必须**显式报错**（绝不静默覆盖），归档原样保留
        assert!(chat_session_restore(sid.to_string()).is_err(), "活跃区有同名档时恢复应报冲突");
        assert!(
            chat_archived_list().iter().any(|s| s.session_id == sid),
            "冲突时归档条目不能被动过"
        );
        // 清掉活跃区那份，验证从归档区恢复正常路径仍通
        chat_archive_delete(sid.to_string()).expect("clean active");

        // ⑥ 从归档区彻底删除（幂等）
        chat_session_purge(sid.to_string()).expect("purge");
        chat_session_purge(sid.to_string()).expect("purge again (idempotent)");
        assert!(chat_archived_list().iter().all(|s| s.session_id != sid));

        // ⑤ 活跃区没文件、归档区也没有 → 才是真的「没聊过」，拒绝白归档
        assert!(
            chat_session_archive("sess-never-existed".to_string(), "x".into(), None).is_err(),
        );
    }
}
