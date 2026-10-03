//! 工作台「任务」持久化 —— 每个任务绑一个文件夹，落盘 `~/.uking/tasks.json`。
//!
//! ## 为什么落盘成单一 JSON
//! 三个任务来源（应用内选文件夹 / 右键「用 U-King 打开」/ 最近任务列表）统一写进这份文件。
//! 重启后最近任务还在，右键打开的目录也自动 upsert 成任务。
//!
//! ## IM 预留（这版不做微信，但口子留好）
//! `Task` 带 `status` / `assignee` / `external_ref` / `source`。将来的微信网关进程只要读写
//! `~/.uking/tasks.json` 就能查询任务状态、指派任务，**不用动客户端一行代码**。
//! 所以这些字段现在就持久化，UI 暂时只用 `status` 染色。
//!
//! 纯 std + serde_json，照抄 device.rs 的 `~/.uking/` 落盘范式，零新依赖。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    /// 任务唯一 id（前端生成或后端补；当前由前端按文件夹生成）
    pub id: String,
    /// 显示名（默认取文件夹名，可重命名）
    pub name: String,
    /// 绑定的文件夹绝对路径
    pub dir: String,
    /// 状态：idle | running | waiting_input | done | error（气泡染色 + IM 查询）
    #[serde(default = "default_status")]
    pub status: String,
    /// 来源：manual | context_menu | im
    #[serde(default = "default_source")]
    pub source: String,
    /// IM 预留：指派给谁（微信用户 id 等）
    #[serde(default)]
    pub assignee: Option<String>,
    /// IM 预留：外部消息 / 会话 id
    #[serde(default)]
    pub external_ref: Option<String>,
    /// 最近打开时间（毫秒，排序用）
    #[serde(default)]
    pub last_opened_at: i64,
    /// 创建时间（毫秒）
    #[serde(default)]
    pub created_at: i64,
    /// Phase 7：工具型会话绑的工具（claude/openclaw…）；任务型为 None
    #[serde(default)]
    pub tool: Option<String>,
    /// Phase 7：启动命令（如 "openclaw gateway run"）
    #[serde(default)]
    pub startup_cmd: Option<String>,
    /// Phase 7：task | tool（default task）
    #[serde(default = "default_kind")]
    pub kind: String,
    /// 手动拖拽排序权重（1-based）。由 `reorder_tasks` 整体赋值；新建=0 → 自动冒顶。
    #[serde(default)]
    pub order: i64,
    /// AI 专家 id（此会话由某专家「召唤」而来）；普通会话为 None。
    #[serde(default)]
    pub expert: Option<String>,
}

fn default_status() -> String {
    "idle".into()
}
fn default_source() -> String {
    "manual".into()
}
fn default_kind() -> String {
    "task".into()
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct TasksFile {
    version: u32,
    tasks: Vec<Task>,
}

fn uking_home() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".uking")
}

fn tasks_path() -> PathBuf {
    uking_home().join("tasks.json")
}

/// 当前毫秒时间戳（i64）。文件不存在等异常时返回 0。
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn read_file() -> TasksFile {
    std::fs::read_to_string(tasks_path())
        .ok()
        .and_then(|s| serde_json::from_str::<TasksFile>(&s).ok())
        .unwrap_or(TasksFile {
            version: 1,
            tasks: Vec::new(),
        })
}

/// 只读模式开关。**默认关**（写照旧），由组合根 `lib.rs` 在本进程是「并行调试实例」时打开。
///
/// 🔴 为什么是注入而不是去问 `instance` 模块：模块独立铁律禁止模块之间横向 import
/// （`check-module-coupling` 当场拦下过这一版）。`tasks.rs` 不需要认识「并行实例」这个概念，
/// 它只需要知道「这轮要不要落盘」。
static READONLY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 并行调试实例启动时由 `lib.rs` 调一次。
pub fn set_readonly(on: bool) {
    READONLY.store(on, std::sync::atomic::Ordering::Relaxed);
}

fn write_file(f: &TasksFile) -> Result<(), String> {
    // 🔴 **并行调试实例只读**（见 `instance.rs`）。两个 U-King 并行跑时各有一份内存态，
    // 谁后写谁赢 —— 而宪法 16 明令「最后写入者获胜不许当未声明的默认」。
    // 这里不上乐观并发（为一个临时并行场景不值），取保守方向：调试实例读得到、用得了，
    // 但不落盘，**主实例那份用户正经在用的任务列表一个字节都不会被踩**。
    //
    // 返回 `Ok(())` 而不是报错：调用方全是 `create/rename/delete` 这类界面动作，
    // 报错会弹一个看不懂的红框；真正该说明的地方是顶栏那条常驻横幅。
    // 静默的代价由 `runtime.instance.inspect` 的 `disabled_in_sidecar` 清单顶着。
    if READONLY.load(std::sync::atomic::Ordering::Relaxed) {
        return Ok(());
    }
    let _ = std::fs::create_dir_all(uking_home());
    let s = serde_json::to_string_pretty(f).map_err(|e| format!("序列化任务失败: {e}"))?;
    std::fs::write(tasks_path(), s).map_err(|e| format!("写入 tasks.json 失败: {e}"))
}

/// 列出全部任务。手动排序优先：`order > 0` 按升序（用户拖出来的顺序）；
/// `order == 0`（未排 / 新建）视为置顶，组内再按最近打开倒序——保持「新会话冒顶」的老手感。
#[tauri::command]
pub fn list_tasks() -> Vec<Task> {
    let mut f = read_file();
    f.tasks.sort_by(|a, b| {
        let ka = if a.order == 0 { i64::MIN } else { a.order };
        let kb = if b.order == 0 { i64::MIN } else { b.order };
        ka.cmp(&kb).then(b.last_opened_at.cmp(&a.last_opened_at))
    });
    f.tasks
}

/// 重排任务顺序。传入「全部条目 id 的目标顺序」，按位置给持久化任务赋 `order`（1-based）。
#[tauri::command]
pub fn reorder_tasks(ids: Vec<String>) -> Result<(), String> {
    let mut f = read_file();
    for (i, id) in ids.iter().enumerate() {
        if let Some(t) = f.tasks.iter_mut().find(|t| &t.id == id) {
            t.order = (i as i64) + 1;
        }
    }
    write_file(&f)
}

/// 新增 / 更新一个任务（按 id 去重）。每次 upsert 都刷新 last_opened_at（置顶）。
/// created_at 仅首次写入时设。返回写盘后的该任务。
#[tauri::command]
pub fn upsert_task(mut task: Task) -> Result<Task, String> {
    if task.id.trim().is_empty() || task.dir.trim().is_empty() {
        return Err("任务缺少 id 或 dir".into());
    }
    let now = now_ms();
    task.last_opened_at = now;

    let mut f = read_file();
    if let Some(existing) = f.tasks.iter_mut().find(|t| t.id == task.id) {
        // 保留原 created_at 与手动排序权重 order（前端不回传 order，重新 upsert 不能把拖好的顺序冲掉）
        task.created_at = if existing.created_at > 0 {
            existing.created_at
        } else {
            now
        };
        task.order = existing.order;
        *existing = task.clone();
    } else {
        if task.created_at == 0 {
            task.created_at = now;
        }
        f.tasks.push(task.clone());
    }
    f.version = 1;
    write_file(&f)?;
    Ok(task)
}

/// 删除一个任务（仅从列表移除，不动文件夹本身）。
#[tauri::command]
pub fn remove_task(id: String) -> Result<(), String> {
    let mut f = read_file();
    f.tasks.retain(|t| t.id != id);
    write_file(&f)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 把 `uking_home()` 指进沙箱。本模块认的是 `USERPROFILE`/`HOME`（不认 `UKING_TEST_HOME`），
    /// 所以走 `enter_raw`：只借全进程唯一那把锁 + 出作用域时还原这两个变量，家目录自己指。
    fn sandboxed(tag: &str) -> crate::testsandbox::Sandbox {
        let sb = crate::testsandbox::enter_raw(tag);
        std::env::set_var("USERPROFILE", sb.root());
        std::env::set_var("HOME", sb.root());
        sb
    }

    /// 一条「对话会话」：前端 `addSession` 建的那种（kind=tool，绑 claude）。
    fn chat_session(id: &str, dir: &str) -> Task {
        Task {
            id: id.into(),
            name: "新对话".into(),
            dir: dir.into(),
            status: "idle".into(),
            source: "manual".into(),
            assignee: None,
            external_ref: None,
            last_opened_at: 0,
            created_at: 0,
            tool: Some("claude".into()),
            startup_cmd: Some("claude".into()),
            kind: "tool".into(),
            order: 0,
            expert: None,
        }
    }

    /// 旧版 tasks.json（Phase 7 之前，没有 tool / startup_cmd / kind / order / expert 这几个键）
    /// 必须照样读得出来，且新增语义字段都落在默认值上 —— 老用户升级后任务一条都不能少。
    #[test]
    fn legacy_tasks_json_without_session_fields_still_loads() {
        let sb = sandboxed("tasks-legacy-format");
        std::fs::create_dir_all(sb.root().join(".uking")).unwrap();
        let legacy = r#"{
          "version": 1,
          "tasks": [
            {"id":"sess-tabc-1","name":"demo","dir":"D:/demo","status":"idle","source":"manual",
             "assignee":null,"external_ref":null,"last_opened_at":200,"created_at":100},
            {"id":"sess-tabc-2","name":"demo2","dir":"D:/demo2","last_opened_at":100}
          ]
        }"#;
        std::fs::write(sb.root().join(".uking").join("tasks.json"), legacy).unwrap();

        let got = list_tasks();
        assert_eq!(got.len(), 2, "旧格式任务不许丢: {got:?}");
        for t in &got {
            assert_eq!(t.kind, "task", "旧任务 kind 缺省必须是 task: {t:?}");
            assert!(t.tool.is_none() && t.startup_cmd.is_none() && t.expert.is_none(), "{t:?}");
            assert_eq!(t.order, 0, "{t:?}");
        }
        // 缺 status/source 的那条走缺省值，而不是整份文件解析失败
        let second = got.iter().find(|t| t.id == "sess-tabc-2").unwrap();
        assert_eq!((second.status.as_str(), second.source.as_str()), ("idle", "manual"));
    }

    /// 「新建对话」落盘 → 读回，kind / tool / startup_cmd / name 一个不丢；
    /// 改名（同 id 再 upsert）仍是一行，created_at 保留；remove_task 能删干净。
    #[test]
    fn chat_session_roundtrips_rename_and_remove() {
        let _sb = sandboxed("tasks-chat-session-roundtrip");
        let saved = upsert_task(chat_session("sess-tool-claude-mgabc123x9z1", "D:/demo")).unwrap();
        assert!(saved.created_at > 0, "首次写入要补 created_at");

        let got = list_tasks();
        assert_eq!(got.len(), 1);
        let t = &got[0];
        assert_eq!(t.id, "sess-tool-claude-mgabc123x9z1");
        assert_eq!(t.kind, "tool");
        assert_eq!(t.tool.as_deref(), Some("claude"));
        assert_eq!(t.startup_cmd.as_deref(), Some("claude"));
        assert_eq!(t.name, "新对话");
        assert_eq!(t.dir, "D:/demo");

        // 改名：前端 renameTask 用 `{ ...cur, name }` 整条回写
        let mut renamed = t.clone();
        renamed.name = "修登录 bug".into();
        let again = upsert_task(renamed).unwrap();
        assert_eq!(again.created_at, saved.created_at, "改名不许重置 created_at");
        let got = list_tasks();
        assert_eq!(got.len(), 1, "同 id 改名必须原位更新，不许多出一行");
        assert_eq!(got[0].name, "修登录 bug");
        assert_eq!(got[0].tool.as_deref(), Some("claude"), "改名不许丢 tool");
        assert_eq!(got[0].kind, "tool");

        remove_task("sess-tool-claude-mgabc123x9z1".into()).unwrap();
        assert!(list_tasks().is_empty(), "关闭/归档走 remove_task，必须能删掉新落盘的会话");
    }

    /// 同 id upsert 按 id 去重（既有语义，也是「重启后计数器归零 → 新会话覆盖旧会话」的机理）；
    /// 前端改用跨重启唯一的 id 之后，同文件夹下的多个对话必须各占一行、互不覆盖。
    #[test]
    fn same_id_dedupes_but_distinct_ids_in_one_folder_coexist() {
        let _sb = sandboxed("tasks-id-dedupe");
        let mut old = chat_session("sess-tool-claude-1", "D:/demo");
        old.name = "上一段对话".into();
        upsert_task(old).unwrap();

        // 旧计数器的撞号场景：同 id → 覆盖，只剩一行（这是后端的既有语义，不改）
        let mut collide = chat_session("sess-tool-claude-1", "D:/demo");
        collide.name = "重启后的新对话".into();
        upsert_task(collide).unwrap();
        let got = list_tasks();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].name, "重启后的新对话", "同 id 就是覆盖 —— 所以前端 id 必须唯一");

        // 新 id 格式：同文件夹再开一个，两行并存，旧的那行原样还在
        let mut fresh = chat_session("sess-tool-claude-mgabc123x9z1", "D:/demo");
        fresh.name = "又一个对话".into();
        upsert_task(fresh).unwrap();
        let got = list_tasks();
        assert_eq!(got.len(), 2, "{got:?}");
        assert!(got.iter().any(|t| t.id == "sess-tool-claude-1" && t.name == "重启后的新对话"));
        assert!(got.iter().any(|t| t.id == "sess-tool-claude-mgabc123x9z1" && t.name == "又一个对话"));
    }

    /// 「对话会话 vs 临时工具会话」的后端侧底线：没有文件夹的拒收（前端 `isDurableSession` 与之同口径），
    /// 被拒也不能把已有的任务文件写坏。
    #[test]
    fn empty_dir_is_rejected_and_leaves_file_intact() {
        let _sb = sandboxed("tasks-empty-dir");
        upsert_task(chat_session("sess-tool-claude-keep", "D:/demo")).unwrap();
        assert!(upsert_task(chat_session("sess-tool-openclaw-gw", "")).is_err());
        assert!(upsert_task(chat_session("sess-tool-openclaw-gw", "   ")).is_err());
        let got = list_tasks();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].id, "sess-tool-claude-keep");
    }
}
