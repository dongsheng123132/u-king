//! 工作台 —— 给客户那个乱文件夹一份**约定**，再把约定编译成一份给 AI 的说明书。
//!
//! ## 工作台是什么（定调见 2026-08-10）
//! 工作台 = ①目录约定（纯文件夹 + markdown）②一组技能 ③一份给 AI 的说明书。
//! **不是一个 React 项目** —— 那样我们要养 N 个前端仓库、客户还改不动，是净负债。
//! 客户唯一改得动的东西就是文件夹和 markdown，所以它就长成那个样子。
//!
//! ## 🔴 现搭，不是从预置模板里挑
//! 每个客户干的活不一样，预置模板换个职业就不合身，而**「不合身但看着像能用」比没有更糟**。
//! 所以 manifest 由技能 `uking-workbench` 按这个客户现写（先 [`scan`] 只读盘点他的文件夹拿事实，
//! 再问不超过 4 句），内置的 `creator` 只是**给 AI 看形状的样例** + 给「什么都不想说」的客户一个落点。
//!
//! 由此多出一段 [`validate`]：以前 manifest 来自我们仓库里那个文件、形状天然对；
//! 现在它是 **AI 现写的**，而写坏的后果全落在客户硬盘上。校验必须在这儿 ——
//! 写在提示词里的「别写 `..`」，模型有一天就是会写。
//!
//! ## 为什么是「编译」不是「拷一份模板文件夹」
//! 客户机上那份 `WORKBENCH.md`（AI 进这个文件夹第一件要读的东西）跟真实目录结构
//! **必须永远一致**。手写两份一定会漂（宪法第 8 条），而这个漂法特别坏：AI 照着过时的
//! 说明书往不存在的目录里写东西，客户看到的是「AI 乱放文件」。所以结构只写在
//! `workbench.json` 里，说明书从它生成。同一条规矩已经在 `llms.txt` 上用过。
//!
//! ## 🔴 为什么落盘在 Rust 不在技能脚本里
//! 这套原本是 `scripts/gen-workbench.mjs`，只有开发机跑得到 —— 客户机上没有那个仓库。
//! 把它照抄进 Rust 就是同一件事两份实现（宪法第 8 条），所以脚本已删：
//! **Rust 独占落盘，开发时跑的就是客户跑的那份代码**（`action run runtime.workbench.install`）。
//!
//! 技能 `uking-workbench` 因此是**全仓唯一一个不带脚本的技能包**，只有方法：
//! 怎么盘点、该问哪几句、manifest 怎么写才合格 —— 然后调这三个动作。
//! 闸门和校验都在这儿而不在那份 md 里，因为提示词管不住模型，代码管得住。
//!
//! **独立可插拔模块**：纯 std + serde_json，只用公共层的 `installer::user_home_dir()`，
//! 不 import 任何其它功能模块，也不碰 `AppHandle`。删它只动 lib.rs。

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// 这个文件同时是「机器读的契约」和「这目录是我们建的」的标记 —— 一个东西干两件事，别再多一个 marker。
pub const MARKER: &str = ".uking-workbench.json";

/// 内嵌**样例**（不是「我们发给客户的模板」）。
///
/// 🔴 定位在 2026-08-10 改过一次，别再退回去：工作台要**按客户自己的实际使用情况现搭**
/// （他产出什么、给谁、哪一步最烦、文件夹里真实有什么），不是从一排预置模板里挑一个。
/// 预置模板换个职业就不合身，而「不合身但看着像能用」比没有更糟。
/// 所以这里留的是**给 AI 看形状**的参考（`uking-workbench` 技能照着它学，不照抄内容），
/// 顺带让「什么都不想说，先给我一个」的客户有个落点。
///
/// 加第二个之前先问「这个我们自己用了几周」—— 模板市场的死法是 10 个半成品。
const TEMPLATES: &[(&str, &str)] = &[(
    "creator",
    include_str!("../workbenches/creator/workbench.json"),
)];

/// 目录数上限。多了客户记不住，记不住就不会用，不会用就退回原来那个乱文件夹。
const MAX_DIRS: usize = 6;

/// 入口指针文件：`(文件名, 谁会自动读它, 另一份在哪)`。
///
/// 🔴 **这是 2026-08-11 补的，补的是一个「装了等于没装」的洞。**
/// 在此之前我们只落 `WORKBENCH.md`，而**没有任何 AI CLI 会自动读这个文件名** ——
/// 客户搭完工作台，AI 进去照样一问三不知，除非他每次手动说「先读 WORKBENCH.md」。
/// 那正是客户反馈的「AI 记忆混乱、每个项目的要求记不住」。
///
/// 各家自动加载的是这两个名字（本机实证，别从文档猜）：
/// - `AGENTS.md` —— Hermes（`hermes --help` 里 `--ignore-rules` 原文：
///   「Skip auto-injection of **AGENTS.md**, SOUL.md, .cursorrules, memory, and preloaded skills」）
///   和 Codex CLI 都读它。
/// - `CLAUDE.md` —— Claude Code 读它。
///
/// **它们是短指针，不是 `WORKBENCH.md` 的副本**：同一份事实抄三遍就会漂三份（宪法第 8 条），
/// 而这个漂法特别坏 —— AI 照着过时的入口往不存在的目录写东西。全文只有一份，
/// 入口只负责把 AI 引过去。这也是「一个项目一个文件夹一份约定」的隔离办法：
/// 记忆不混，是因为约定根本不在全局那一份里。
const ENTRYPOINTS: &[(&str, &str, &str)] = &[
    ("AGENTS.md", "Codex CLI / Hermes", "CLAUDE.md"),
    ("CLAUDE.md", "Claude Code", "AGENTS.md"),
];

/// 入口文件已存在时的说法。**不能跟普通文件用同一句** ——
/// 默默跳过一个客户自己写的 `CLAUDE.md`，后果是整个工作台约定没人读，而他以为装好了。
const SKIP_ENTRY: &str = "已存在（可能是你自己或别的 AI 工具写的），没覆盖 —— \
     🔴 里面若没提到 WORKBENCH.md，AI 进来不会自动读这个工作台的约定；\
     勾上「更新说明书」可以让我们重写它";

/// 计划里的一条。**先全部算出来再落盘** —— 这样「预览」打印的就是真的要干的事，
/// 而不是另写一套模拟逻辑（那就是第二份实现，会跟真的漂开）。
#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    /// `dir` | `file`
    pub kind: &'static str,
    /// 相对工作台根的路径
    pub rel: String,
    /// 文件内容；目录为 None
    pub body: Option<String>,
    /// `create` 新建 · `skip` 已存在不动 · `update` 覆盖（只有说明书 + 显式要求时）
    pub verdict: &'static str,
    /// 为什么是这个 verdict —— 给人看的一句话
    pub why: String,
}

impl Step {
    fn to_json(&self) -> Value {
        json!({ "kind": self.kind, "path": self.rel, "verdict": self.verdict, "why": self.why })
    }
}

fn parse(raw: &str) -> Result<Value, String> {
    serde_json::from_str(raw).map_err(|e| format!("模板 JSON 坏了: {e}"))
}

/// 取一个模板的 manifest。
pub fn manifest(id: &str) -> Result<Value, String> {
    TEMPLATES
        .iter()
        .find(|(k, _)| *k == id)
        .ok_or_else(|| format!("没有这个模板：{id}（可用：{}）", ids().join(", ")))
        .and_then(|(_, raw)| parse(raw))
}

pub fn ids() -> Vec<String> {
    TEMPLATES.iter().map(|(k, _)| (*k).to_string()).collect()
}

/// 决定这次要装的到底是哪一份：AI 现搭的 `manifest` 优先，没给才回落内置样例。
///
/// 🔴 **`known_skills` 由组合根 lib.rs 注入**（`skillpack::names()`），本模块**不 import
/// skillpack** —— 模块铁律②：新模块之间禁止互相 import。同 `metrics` 拿 `rtk::is_active()`
/// 的手法。
pub fn resolve(
    id: Option<&str>,
    inline: Option<&Value>,
    known_skills: &[String],
) -> Result<Value, String> {
    let wb = match inline {
        Some(v) if !v.is_null() => v.clone(),
        _ => manifest(id.unwrap_or_else(|| TEMPLATES[0].0))?,
    };
    validate(&wb, known_skills)?;
    Ok(wb)
}

/// 校验一份 manifest。
///
/// 🔴 **这一段是现搭之后新增的，不是可选的**：以前 manifest 来自我们仓库里那个文件，
/// 形状天然对；现在它是 **AI 现写的**，而写坏的后果全落在客户硬盘上 ——
/// `path` 里一个 `..` 就写到工作台外面去了，缺 `rule` 生成出来的说明书就只有目录名没有约定
/// （等于没有约定），编一个不存在的技能名客户点了没反应。
///
/// **校验必须在这儿，不能只写在 SKILL.md 里**：提示词里写「别写 `..`」，模型有一天就是会写。
fn validate(wb: &Value, known_skills: &[String]) -> Result<(), String> {
    let mut bad: Vec<String> = Vec::new();
    for k in ["id", "name", "one_liner", "for_whom"] {
        if wb[k].as_str().map(|s| s.trim().is_empty()).unwrap_or(true) {
            bad.push(format!("缺 {k}"));
        }
    }

    let dirs = wb["dirs"].as_array().cloned().unwrap_or_default();
    if dirs.is_empty() {
        bad.push("dirs 为空 —— 没有目录约定就不叫工作台".into());
    } else if dirs.len() > MAX_DIRS {
        bad.push(format!(
            "dirs 有 {} 个，上限 {MAX_DIRS} —— 多了客户记不住就不会用，不会用就退回原来那个乱文件夹",
            dirs.len()
        ));
    }
    let mut seen: Vec<String> = Vec::new();
    for d in &dirs {
        let p = d["path"].as_str().unwrap_or_default().trim().to_string();
        let label = if p.is_empty() { "(没写 path 的那个)".to_string() } else { p.clone() };
        for k in ["path", "label", "purpose", "rule", "naming"] {
            if d[k].as_str().map(|s| s.trim().is_empty()).unwrap_or(true) {
                // `rule` 和 `naming` 一样是硬要求：光有目录名等于没有约定
                bad.push(format!("目录 {label} 缺 {k}"));
            }
        }
        if !p.is_empty() {
            // 🔴 逃逸检查：`..` / 绝对路径 / 盘符 —— 一个都不许，否则写到工作台外面去了
            let norm = p.replace('\\', "/");
            if norm.starts_with('/')
                || norm.split('/').any(|seg| seg == "..")
                || Path::new(&p).is_absolute()
                || norm.chars().nth(1) == Some(':')
            {
                bad.push(format!("目录 {label} 不许是绝对路径、带 .. 或带盘符 —— 会写到工作台外面去"));
            }
            if seen.contains(&norm.to_lowercase()) {
                bad.push(format!("目录 {label} 重复了"));
            }
            seen.push(norm.to_lowercase());
        }
    }

    // 编一个客户机上不存在的技能名 = 他照着说明书喊一句，什么都不会发生
    for s in wb["skills"].as_array().cloned().unwrap_or_default() {
        let sid = s["id"].as_str().unwrap_or_default().to_string();
        if sid.is_empty() {
            bad.push("skills 里有一条没写 id".into());
        } else if !known_skills.iter().any(|k| k == &sid) {
            bad.push(format!(
                "技能 {sid} 在这台机器上不存在（U-King 发的是：{}）",
                known_skills.join(", ")
            ));
        }
    }

    // 「没有什么」跟「有什么」一样重要 —— 空着等于默认什么都行，客户用两天才发现
    if wb["not_included"].as_array().map(|a| a.is_empty()).unwrap_or(true) {
        bad.push("not_included 为空 —— 必须写清这个工作台做不到什么".into());
    }

    if bad.is_empty() {
        Ok(())
    } else {
        Err(format!("工作台定义不合格：\n - {}", bad.join("\n - ")))
    }
}

/// 只读盘点客户**现在那个乱文件夹**里到底有什么 —— 「按他的实际使用情况搭」的事实来源。
///
/// 🔴 **只 `stat`，一个文件的内容都不读**。AI 拿到这份 JSON 就够定目录了，而它还不知道
/// 哪些文件是隐私。要读某几个，得客户点名。
///
/// 🔴 **不碰他机器上的使用记录**（别家 AI 的会话、用量统计、行为时间轴）。那些数据 U-King
/// 有，但拿它替客户做决定是越界的 —— 依据只能是他**主动指的这个文件夹** + 他**亲口说的话**。
///
/// 上限是硬的：客户的「下载」目录可能有十万个文件，扫穿了这条只读命令就成了他机器上的一次卡顿。
/// 撞到上限就把 `truncated` 报出来 —— **「没看到」不等于「没有」**。
pub fn scan(root: &Path) -> Result<Value, String> {
    const MAX_FILES: usize = 20_000;
    const MAX_DEPTH: usize = 6;
    if !root.is_dir() {
        return Err(format!("{} 不是一个目录", root.display()));
    }

    let mut by_ext: Vec<(String, usize)> = Vec::new();
    let mut top_dirs: Vec<(String, usize)> = Vec::new();
    let mut recent_sample: Vec<Value> = Vec::new();
    let (mut files, mut bytes, mut recent, mut deepest) = (0usize, 0u64, 0usize, 0usize);
    let mut truncated = false;
    let now = std::time::SystemTime::now();

    let bump = |v: &mut Vec<(String, usize)>, k: String| match v.iter_mut().find(|(n, _)| *n == k) {
        Some(e) => e.1 += 1,
        None => v.push((k, 1)),
    };

    // 显式栈而不是递归：客户目录深度不可控，递归爆栈会让一条只读命令把进程带走
    let mut stack: Vec<(PathBuf, String, usize)> = vec![(root.to_path_buf(), String::new(), 0)];
    while let Some((abs, rel, depth)) = stack.pop() {
        if files >= MAX_FILES {
            truncated = true;
            break;
        }
        if depth > MAX_DEPTH {
            truncated = true;
            continue;
        }
        deepest = deepest.max(depth);
        let Ok(entries) = std::fs::read_dir(&abs) else { continue };
        for e in entries.filter_map(|e| e.ok()) {
            let name = e.file_name().to_string_lossy().to_string();
            // 隐藏目录和依赖目录不是客户的活
            if name.starts_with('.') || name == "node_modules" {
                continue;
            }
            let child_rel = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
            if e.path().is_dir() {
                stack.push((e.path(), child_rel, depth + 1));
                continue;
            }
            if files >= MAX_FILES {
                truncated = true;
                break;
            }
            let Ok(md) = e.metadata() else { continue }; // 只 stat，**不读内容**
            files += 1;
            bytes += md.len();
            let ext = std::path::Path::new(&name)
                .extension()
                .map(|x| format!(".{}", x.to_string_lossy().to_lowercase()))
                .unwrap_or_else(|| "(无扩展名)".into());
            bump(&mut by_ext, ext);
            let top = child_rel.split('/').next().unwrap_or("(根目录)").to_string();
            let top = if child_rel.contains('/') { top } else { "(根目录)".to_string() };
            bump(&mut top_dirs, top);
            if let Ok(age) = md.modified().and_then(|m| now.duration_since(m).map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::Other, "mtime in future")
            })) {
                let days = age.as_secs() / 86_400;
                if days < 30 {
                    recent += 1;
                    if recent_sample.len() < 15 {
                        recent_sample.push(json!({ "path": child_rel, "days_ago": days }));
                    }
                }
            }
        }
    }

    let top = |mut v: Vec<(String, usize)>, n: usize| {
        v.sort_by(|a, b| b.1.cmp(&a.1));
        v.truncate(n);
        v.into_iter().map(|(name, files)| json!({ "name": name, "files": files })).collect::<Vec<_>>()
    };

    Ok(json!({
        "root": root.display().to_string(),
        // ★ 样本量必须报：下面每个分布都是拿它算的（影核观测记账）
        "files": files,
        "total_mb": bytes / 1_048_576,
        "deepest_level": deepest,
        "truncated": truncated,
        "by_ext": top(by_ext, 12),
        "top_dirs": top(top_dirs, 10),
        "changed_in_30d": recent,
        "recent_sample": recent_sample,
        "note": "只 stat 不读内容；隐藏目录和 node_modules 已跳过。truncated=true 时这些分布只代表扫到的那批，不代表全部。",
    }))
}

/// 模板清单（给界面/AI 挑）。不含目录明细，明细在 `manifest`。
pub fn list() -> Vec<Value> {
    TEMPLATES
        .iter()
        .filter_map(|(id, raw)| parse(raw).ok().map(|w| (id, w)))
        .map(|(id, w)| {
            json!({
                "id": id,
                "name": w["name"],
                "one_liner": w["one_liner"],
                "for_whom": w["for_whom"],
                "dirs": w["dirs"].as_array().map(|a| a.len()).unwrap_or(0),
                "skills": w["skills"].as_array().map(|a| {
                    a.iter().filter_map(|s| s["id"].as_str().map(String::from)).collect::<Vec<_>>()
                }).unwrap_or_default(),
                // 🔴 「没有什么」跟「有什么」一样重要：一块「什么都看得见」的板会让人以为这就是全部
                "not_included": w["not_included"],
            })
        })
        .collect()
}

/// 目标目录能不能装。**这三条闸门是这个模块存在的主要理由**（宪法第 10 条：不碰用户真实状态）。
///
/// 返回 `Err` = 不许装，正文就是给客户看的话。
fn gate(target: &Path) -> Result<(), String> {
    // ① 盘根 / 家目录 —— 就算是空的也不许。没有人想把工作台建在 `C:\`。
    //    脚本版没有这条：那时只有开发机手敲，现在 CLI/MCP/AI 都能传路径进来。
    if target.parent().is_none() {
        return Err(format!("{} 是盘根，不能当工作台。在里面新建一个文件夹再来。", target.display()));
    }
    let home = crate::installer::user_home_dir();
    if same_path(target, &home) {
        return Err(format!("{} 是你的用户目录，不能当工作台根。", target.display()));
    }

    // ② 非空且不是我们建的 → 拒绝。**故意没有 --force**：客户最容易随手选中「桌面」或「文档」。
    if target.exists() {
        let entries: Vec<_> = std::fs::read_dir(target)
            .map_err(|e| format!("读不了 {}：{e}", target.display()))?
            .filter_map(|e| e.ok())
            .collect();
        let ours = entries.iter().any(|e| e.file_name() == MARKER);
        if !entries.is_empty() && !ours {
            return Err(format!(
                "{} 不能当工作台根：里面已经有 {} 个东西，而且不是 U-King 建的（没有 {MARKER}）。\n\
                 换一个空目录，或者在它里面新建一个子文件夹当工作台根。\n\
                 这里故意没有「强制覆盖」：装错地方撒一堆文件夹，比装不上难收拾得多。",
                target.display(),
                entries.len()
            ));
        }
    }
    Ok(())
}

fn same_path(a: &Path, b: &Path) -> bool {
    let n = |p: &Path| {
        p.to_string_lossy()
            .replace('\\', "/")
            .trim_end_matches('/')
            .to_lowercase()
    };
    n(a) == n(b)
}

/// 算出「要干什么」。**只读，一个字节都不写** —— 界面上的「预览」和真安装走同一份计划。
pub fn plan(wb: &Value, target: &Path, overwrite_doc: bool) -> Result<Vec<Step>, String> {
    gate(target)?;

    let mut out: Vec<Step> = Vec::new();
    push_dir(&mut out, target, ".");
    let dirs = wb["dirs"].as_array().cloned().unwrap_or_default();
    for d in &dirs {
        let p = d["path"].as_str().unwrap_or_default().to_string();
        if p.is_empty() {
            return Err("模板里有一个目录没写 path".into());
        }
        push_dir(&mut out, target, &p);
        push_file(
            &mut out,
            target,
            &format!("{p}/README.md"),
            dir_readme(d, wb),
            false,
            SKIP_DEFAULT,
        );
    }
    // 说明书 + 契约：这两个是**我们生成的**，可以更新；但也只在显式要求时更新。
    push_file(&mut out, target, "WORKBENCH.md", compile_doc(wb), overwrite_doc, SKIP_DEFAULT);
    push_file(
        &mut out,
        target,
        MARKER,
        serde_json::to_string_pretty(&wb).unwrap_or_default() + "\n",
        overwrite_doc,
        SKIP_DEFAULT,
    );
    // 入口指针：没有这两个文件，上面那份说明书就没人读（见 ENTRYPOINTS 的注释）。
    for (name, _, _) in ENTRYPOINTS {
        let hint = if entry_is_wired(target, name) {
            // 我们上次装的，或者他自己写了句指过去的 —— 两种都已经接上了，别吓唬人。
            "已存在且已指向 WORKBENCH.md，不动"
        } else {
            SKIP_ENTRY
        };
        push_file(&mut out, target, name, compile_entry(wb, name), overwrite_doc, hint);
    }
    Ok(out)
}

fn push_dir(out: &mut Vec<Step>, target: &Path, rel: &str) {
    let abs = if rel == "." { target.to_path_buf() } else { target.join(rel) };
    let exists = abs.is_dir();
    out.push(Step {
        kind: "dir",
        rel: rel.to_string(),
        body: None,
        verdict: if exists { "skip" } else { "create" },
        why: if exists { "已有这个目录".into() } else { "新建".into() },
    });
}

/// `skip_hint` = 已存在而跳过时那句话。入口文件要单独说 —— 默默跳过一个 `CLAUDE.md`
/// 的后果不是「少了个文件」，是**整个工作台约定没人读**，客户却以为装好了。
fn push_file(
    out: &mut Vec<Step>,
    target: &Path,
    rel: &str,
    body: String,
    may_overwrite: bool,
    skip_hint: &str,
) {
    let exists = target.join(rel).exists();
    let (verdict, why) = if !exists {
        ("create", "新建".to_string())
    } else if may_overwrite {
        ("update", "你要求更新说明书".to_string())
    } else {
        // 🔴 客户可能改过它。README 是他的东西，不是我们的。
        ("skip", skip_hint.to_string())
    };
    out.push(Step { kind: "file", rel: rel.to_string(), body: Some(body), verdict, why });
}

/// 已存在而跳过时的默认说法。
const SKIP_DEFAULT: &str = "已存在，不覆盖（要更新说明书就勾上「更新说明书」）";

/// 这个入口文件**接上了吗** —— 即 AI 读了它之后会不会被指到 `WORKBENCH.md`。
///
/// 🔴 判据是「能不能读到约定」，**不是「是不是我们生成的」**。差别是实打实的：
/// - 我们上次自己装的那份被跳过 = 幂等，正常，不该报警（第一版就是在这儿误报的）；
/// - 客户在他自己的 `CLAUDE.md` 里手写了一句「约定见 WORKBENCH.md」= 也接上了，同样不该报警；
/// - 只有「有这个文件、但里面压根没提 WORKBENCH.md」才是真的断了。
///
/// 读不出来（权限/编码）按**断了**算 —— 拿不准时宁可多喊一句，也别让客户以为装好了。
fn entry_is_wired(target: &Path, name: &str) -> bool {
    let p = target.join(name);
    if !p.exists() {
        return false;
    }
    std::fs::read_to_string(&p)
        .map(|s| s.contains("WORKBENCH.md"))
        .unwrap_or(false)
}

/// 真装。幂等：重跑只补缺的，他改过的文件一个字节不动。
pub fn install(wb: &Value, target: &Path, overwrite_doc: bool) -> Result<Value, String> {
    let steps = plan(wb, target, overwrite_doc)?;
    let (mut created, mut skipped, mut updated) = (0u32, 0u32, 0u32);

    for s in &steps {
        let abs = if s.rel == "." { target.to_path_buf() } else { target.join(&s.rel) };
        match s.verdict {
            "skip" => {
                skipped += 1;
                continue;
            }
            "update" => updated += 1,
            _ => created += 1,
        }
        if s.kind == "dir" {
            std::fs::create_dir_all(&abs).map_err(|e| format!("建不了目录 {}：{e}", abs.display()))?;
        } else {
            if let Some(parent) = abs.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("建不了目录 {}：{e}", parent.display()))?;
            }
            std::fs::write(&abs, s.body.clone().unwrap_or_default())
                .map_err(|e| format!("写不了 {}：{e}", abs.display()))?;
        }
    }

    // 🔴 装完之后**回头量一遍真实世界**，不是从计划推断。
    // 入口没接上 = 这次「装好了」是假的：AI 进来读不到任何约定，而客户看到 ok:true 就以为成了，
    // 然后回来说「你们这玩意还是不记事」。报告对、世界坏，是我们踩过的坑。
    let stale: Vec<String> = ENTRYPOINTS
        .iter()
        .filter(|(n, _, _)| !entry_is_wired(target, n))
        .map(|(n, r, _)| format!("`{n}`（{r} 进这个文件夹时读的那份）里没提 WORKBENCH.md —— AI 读不到本工作台的约定"))
        .collect();

    let next = if stale.is_empty() {
        format!(
            "直接在这个文件夹里开工就行 —— AI 进来会自动读 AGENTS.md / CLAUDE.md，\
             被指到 WORKBENCH.md：cd \"{}\" && claude（或 hermes / codex）",
            target.display()
        )
    } else {
        format!(
            "⚠️ 入口文件是你自己的，我们没覆盖 —— 请在里面加一句「本文件夹的约定见 WORKBENCH.md」，\
             否则 AI 进来读不到工作台约定。或者重装时勾上「更新说明书」。目录：{}",
            target.display()
        )
    };

    Ok(json!({
        "ok": true,
        "workbench": wb["id"].as_str().unwrap_or_default(),
        "path": target.display().to_string(),
        "created": created,
        "updated": updated,
        "skipped": skipped,
        "steps": steps.iter().map(Step::to_json).collect::<Vec<_>>(),
        "warnings": stale,
        "next": next,
    }))
}

/// 看一眼：有哪些模板，以及（给了路径的话）这个路径现在是什么状况、装的话会干什么。
/// **预览就是 plan 本身**，不是另写一套模拟。
pub fn inspect(wb: &Value, target: Option<&Path>, overwrite_doc: bool, skill_in: &[String]) -> Value {
    // ★ 可用性约定：描述「能力」的只读动作必须回答**能不能用**，不是**装没装**。
    //
    // 🔴 这里 `ready` 问的是**「按客户实际情况现搭」这条路通不通** —— 那条路要靠
    // `uking-workbench` 技能（方法在它里面：怎么盘点、问哪几句、manifest 怎么写）。
    // 技能没装进任何 AI 时，入口还在（起手词 / 新建项目那一问都点得动），
    // AI 却不知道该怎么搭 —— 大概率自己 mkdir 一通，把这儿所有闸门全绕过去。
    // **报告是对的、世界是坏的**，正是 Token 压缩机踩过的那个坑。
    //
    // 装内置样例那条路**不依赖技能**（纯 Rust），所以 blockers 里要把这句一起说清楚，
    // 否则客户会以为整个功能都废了。
    let ready = !skill_in.is_empty();
    let blockers: Vec<String> = if ready {
        Vec::new()
    } else {
        vec![
            "「按你的情况现搭」要靠 uking-workbench 技能，它还没装进这台机器上的任何 AI —— \
             去「AI 技能 / 上手」点一下「装进我的 AI」。（装内置样例不受影响，那条路不用技能。）"
                .to_string(),
        ]
    };
    let mut v = json!({
        "templates": list(),
        "default_template": TEMPLATES[0].0,
        "ready": ready,
        "blockers": blockers,
        // 报事实不报感觉：装在哪几个 AI 里，列出来
        "skill_installed_in": skill_in,
    });
    let Some(t) = target else { return v };

    let is_wb = t.join(MARKER).exists();
    let empty = !t.exists()
        || std::fs::read_dir(t).map(|mut d| d.next().is_none()).unwrap_or(false);
    match plan(wb, t, overwrite_doc) {
        Ok(steps) => {
            v["target"] = json!({
                "path": t.display().to_string(),
                "exists": t.exists(),
                "empty": empty,
                "is_workbench": is_wb,
                "installable": true,
                "blockers": [],
                "plan": steps.iter().map(Step::to_json).collect::<Vec<_>>(),
            });
        }
        Err(e) => {
            v["target"] = json!({
                "path": t.display().to_string(),
                "exists": t.exists(),
                "empty": empty,
                "is_workbench": is_wb,
                "installable": false,
                "blockers": [e],
                "plan": [],
            });
        }
    }
    v
}

/// 每个目录里放一句人话 —— 客户在 Explorer 里点进来，不该看到一个空文件夹让他猜。
fn dir_readme(d: &Value, wb: &Value) -> String {
    let fed = d["fed_by"]
        .as_array()
        .filter(|a| !a.is_empty())
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str())
                .map(|s| format!("`{s}`"))
                .collect::<Vec<_>>()
                .join(" / ")
        })
        .unwrap_or_else(|| "（不靠技能，AI 直接写）".into());
    let s = |k: &str| d[k].as_str().unwrap_or("").to_string();
    format!(
        "# {}\n\n{}\n\n**规矩**：{}\n\n**命名**：{}\n\n**谁往这儿放东西**：{fed}\n\n---\n\
         本文件由 `{}` 模板生成。目录含义的真相源是根目录的 `{MARKER}`，改结构请改那里再重新生成。\n",
        s("label"),
        s("purpose"),
        s("rule"),
        s("naming"),
        wb["id"].as_str().unwrap_or("")
    )
}

/// 目录表 —— `AGENTS.md` / `CLAUDE.md` / `WORKBENCH.md` 共用这一份生成逻辑。
///
/// 三处输出里都有这张表，但**只有这一处代码**：它们全是从 `MARKER` 编译出来的产物，
/// 一起生成、一起更新，不存在「其中一份手改了另两份不知道」（宪法第 8 条防的是那个，
/// 不是「同一个编译器输出多个目标」）。公共能力复用不复制 —— 模块铁律③。
fn dirs_table(wb: &Value) -> Vec<String> {
    let mut l = vec![
        "| 目录 | 是什么 | 规矩 | 命名 |".to_string(),
        "|---|---|---|---|".to_string(),
    ];
    for d in wb["dirs"].as_array().cloned().unwrap_or_default() {
        let g = |k: &str| d[k].as_str().unwrap_or("").to_string();
        l.push(format!(
            "| `{}/` | {}：{} | {} | {} |",
            g("path"),
            g("label"),
            g("purpose"),
            g("rule"),
            g("naming")
        ));
    }
    l
}

/// 编译一份**入口文件**（`AGENTS.md` / `CLAUDE.md`）。
///
/// 各家 AI CLI 进目录会自动加载这两个名字，所以真正的第一入口是它们，不是 `WORKBENCH.md`。
///
/// ## 🔴 它为什么带目录表，而不是一句「去读 WORKBENCH.md」
/// 第一版真就是纯指针（2026-08-11 上午），**当天实测被推翻**：让 Hermes 进一个刚装好的
/// 工作台问「03-选题 的规矩是什么」，它明说「上下文里的 AGENTS.md」——
/// 说明自动注入这条前提是对的 —— 但它**没有跟着指针去读 `WORKBENCH.md`**，
/// 甚至没 `ls` 一下就断言「当前目录是空的」，还把真实产物当成了「模板」。
///
/// 结论：**指针只能保证 AI 拿到入口，保证不了它去追全文**。写「你必须先读 X」是提示词，
/// 而提示词管不住模型（这条本模块头上就写着，我还是先踩了一次）。
/// 所以最常被问到的事实（有哪些目录、每个目录什么规矩）直接放进入口，零跳转可得；
/// 长而不常用的（动作起手词模板、技能清单、做不到什么）才留在 `WORKBENCH.md`。
///
/// 体积仍然要克制 —— 这个文件每次会话全量进上下文，长一行就贵一次
/// （同 `CLAUDE.md` 自己那条 300 行预算的理由）。目录上限 [`MAX_DIRS`] 顺带也是这张表的上限。
fn compile_entry(wb: &Value, file_name: &str) -> String {
    let s = |k: &str| wb[k].as_str().unwrap_or("").to_string();
    let (reader, other) = ENTRYPOINTS
        .iter()
        .find(|(n, _, _)| *n == file_name)
        .map(|(_, r, o)| (*r, *o))
        .unwrap_or(("AI 工具", ""));

    let mut l: Vec<String> = Vec::new();
    l.push(format!("# {}", s("name")));
    l.push(String::new());
    l.push(s("one_liner"));
    l.push(String::new());
    // 措辞是实测调过的：第一版写「这是一个工作台……本文件只是入口」，
    // Hermes 读完回了句「AGENTS.md 只是入口模板，不是真实产物」——
    // 自我贬低的措辞会让模型把约定当参考资料。现在直说「你正在里面，照着做」。
    l.push("## 🤖 AI 读这里".into());
    l.push(String::new());
    l.push("**你正在一个 U-King 工作台里。下面就是这个文件夹的约定，干活前照着对一遍。**".into());
    l.push(String::new());
    l.push(format!("**给谁用**：{}", s("for_whom")));
    l.push(String::new());

    l.push("## 目录".into());
    l.push(String::new());
    l.push("只往这些目录里放东西，**别新建同义目录**（「素材2」「新建文件夹」都不行）。".into());
    l.push(String::new());
    l.extend(dirs_table(wb));
    l.push(String::new());

    l.push("## 还有什么在 `WORKBENCH.md` 里".into());
    l.push(String::new());
    l.push(
        "常做的动作（起手词模板）、这个工作台配了哪些技能、以及**它明确做不到什么** —— \
         同目录的 `WORKBENCH.md`，需要时读它，别猜。"
            .into(),
    );
    l.push(String::new());
    l.push(format!(
        "`WORKBENCH.md` 和 `{MARKER}` 都是从契约编译出来的，**别手改** —— 改了下次重新生成就没了。\
         要改约定就改结构再重装。"
    ));
    l.push(String::new());
    l.push("---".into());
    l.push(String::new());
    l.push(format!(
        "由 U-King `runtime.workbench.install` 生成，供 {reader} 进这个文件夹时自动加载。{}",
        if other.is_empty() {
            String::new()
        } else {
            format!("同目录的 `{other}` 是给别家 AI 的同一份。")
        }
    ));
    l.push(String::new());
    l.join("\n")
}

/// 给 AI 看的说明书。**全部约定的唯一全文**（入口指针会把 AI 引到这里）。
pub fn compile_doc(wb: &Value) -> String {
    let s = |k: &str| wb[k].as_str().unwrap_or("").to_string();
    let mut l: Vec<String> = Vec::new();
    l.push(format!("# {}", s("name")));
    l.push(String::new());
    l.push(s("one_liner"));
    l.push(String::new());
    l.push(format!("**给谁用**：{}", s("for_whom")));
    l.push(String::new());
    l.push("> 🤖 **AI 读这里**：这是一个 U-King 工作台。下面写的目录含义和规矩就是这个文件夹的全部约定，".into());
    l.push(format!(
        "> 机器可读的那份在 `{MARKER}`。**本文件是从它编译出来的，别手改** —— 改了下次重新生成就没了。"
    ));
    l.push(String::new());

    l.push("## 目录".into());
    l.push(String::new());
    l.extend(dirs_table(wb));
    l.push(String::new());

    l.push("## 常做的动作".into());
    l.push(String::new());
    l.push("这几条既是给人看的起手词，也是界面上那几个按钮的来源（同一份契约，不是两份）。".into());
    l.push(String::new());
    for a in wb["actions"].as_array().cloned().unwrap_or_default() {
        let needs = a["needs"]
            .as_array()
            .filter(|x| !x.is_empty())
            .map(|x| {
                format!(
                    "（要用 {}）",
                    x.iter()
                        .filter_map(|s| s.as_str())
                        .map(|s| format!("`{s}`"))
                        .collect::<Vec<_>>()
                        .join(" / ")
                )
            })
            .unwrap_or_default();
        l.push(format!("### {} {needs}", a["label"].as_str().unwrap_or("")));
        l.push(String::new());
        l.push(a["hint"].as_str().unwrap_or("").into());
        l.push(String::new());
        l.push("```".into());
        l.push(a["prompt"].as_str().unwrap_or("").into());
        l.push("```".into());
        l.push(String::new());
    }

    l.push("## 这个工作台要装的技能".into());
    l.push(String::new());
    for sk in wb["skills"].as_array().cloned().unwrap_or_default() {
        l.push(format!(
            "- `{}` —— {}",
            sk["id"].as_str().unwrap_or(""),
            sk["why"].as_str().unwrap_or("")
        ));
    }
    l.push(String::new());
    l.push("在 U-King 里点「装进我的 AI」会把它们拷进你已装工具各自的 skills 目录（Claude Code / Codex / OpenClaw / Hermes）。".into());
    l.push("也可以自己拷到 `~/.claude/skills/` 下。".into());
    l.push(String::new());

    l.push("## 这个工作台**没有**什么".into());
    l.push(String::new());
    l.push("写在这儿是因为：一块「什么都看得见」的板会让人以为这就是全部。".into());
    l.push(String::new());
    for n in wb["not_included"].as_array().cloned().unwrap_or_default() {
        l.push(format!("- {}", n.as_str().unwrap_or("")));
    }
    l.push(String::new());
    l.push("---".into());
    l.push(String::new());
    l.push(format!(
        "模板 `{}` · schema v{} · 由 U-King 的 `runtime.workbench.install` 生成",
        s("id"),
        wb["schema_version"]
    ));
    l.push(String::new());
    l.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("uking-wb-test-{name}"));
        let _ = std::fs::remove_dir_all(&p);
        p
    }

    /// 内置样例当夹具。**别改成手捏一份** —— 手捏的会跟真实形状漂开，
    /// 而这些用例断言的正是「真实形状装进去会发生什么」。
    fn ex() -> Value {
        manifest("creator").unwrap()
    }

    /// 这台机器上「真有的技能」—— 校验用。测试里给一份固定的，
    /// 免得单测跟着 skillpack 增删包一起红（那不是这个模块的事）。
    fn skills() -> Vec<String> {
        ["uking-web", "uking-browse", "uking-office-read", "uking-office-edit", "uking-pdf",
         "uking-vision", "uking-aigc", "uking-docx", "uking-xlsx", "uking-ppt",
         "uking-cad", "uking-mail", "uking-teamwork", "uking-workbench"]
            .iter().map(|s| s.to_string()).collect()
    }

    /// 拿样例改一处，专门用来验「这一处坏了会不会被拦下」。
    fn broken(mutate: impl FnOnce(&mut Value)) -> Value {
        let mut w = ex();
        mutate(&mut w);
        w
    }

    /// ★ 现搭的 manifest 是 **AI 写的**，写坏的后果全落在客户硬盘上。
    /// 每一条都对应一种真实的写坏方式；断言里连**理由**一起验 ——
    /// 「拒了」不等于「因为对的理由拒的」，只验 is_err 的用例改坏别处照样绿。
    #[test]
    fn validate_rejects_every_way_an_ai_can_write_it_wrong() {
        let ok = ex();
        assert!(validate(&ok, &skills()).is_ok(), "内置样例自己必须过：{:?}", validate(&ok, &skills()));

        let cases: Vec<(&str, Value, &str)> = vec![
            ("路径逃逸 ..", broken(|w| w["dirs"][0]["path"] = json!("../../桌面")), "写到工作台外面"),
            ("绝对路径", broken(|w| w["dirs"][0]["path"] = json!("C:/Windows")), "写到工作台外面"),
            ("目录没写规矩", broken(|w| { w["dirs"][1].as_object_mut().unwrap().remove("rule"); }), "缺 rule"),
            ("目录没写命名", broken(|w| { w["dirs"][1].as_object_mut().unwrap().remove("naming"); }), "缺 naming"),
            ("编了个不存在的技能", broken(|w| w["skills"].as_array_mut().unwrap().push(json!({"id":"uking-lawsuit","why":"编的"}))), "不存在"),
            ("没说做不到什么", broken(|w| w["not_included"] = json!([])), "not_included"),
            ("目录重复", broken(|w| { let p = w["dirs"][0]["path"].clone(); w["dirs"][1]["path"] = p; }), "重复"),
            ("一个目录都没有", broken(|w| w["dirs"] = json!([])), "dirs 为空"),
            ("缺 for_whom", broken(|w| { w.as_object_mut().unwrap().remove("for_whom"); }), "缺 for_whom"),
        ];
        for (name, wb, expect) in cases {
            let e = validate(&wb, &skills()).expect_err(&format!("{name} 必须被拦下"));
            assert!(e.contains(expect), "{name} 拒绝理由不对：期望含「{expect}」，实际 {e}");
        }

        // 目录超上限：单独一条，因为要构造 7 个
        let mut many = ex();
        let one = many["dirs"][0].clone();
        many["dirs"] = json!((0..MAX_DIRS + 1).map(|i| {
            let mut d = one.clone();
            d["path"] = json!(format!("{i:02}-x"));
            d
        }).collect::<Vec<_>>());
        let e = validate(&many, &skills()).expect_err("超过上限必须被拦下");
        assert!(e.contains(&MAX_DIRS.to_string()), "该说清上限是多少：{e}");
    }

    /// 校验必须发生在**写之前**：定义坏了，硬盘上不许留下任何东西。
    /// （闸门挡的是落点，这条挡的是定义 —— 两件事，都得在 handler 之前。）
    #[test]
    fn a_broken_definition_writes_nothing() {
        let t = tmp("badwrite");
        let wb = broken(|w| w["dirs"][0]["path"] = json!("../逃逸"));
        assert!(resolve(None, Some(&wb), &skills()).is_err(), "坏定义必须过不了 resolve");
        assert!(!t.exists(), "被拒之后一个目录都不许建");
    }

    /// `resolve`：给了现搭的就用现搭的，没给才回落内置样例。
    /// 这条是整个「按客户实际情况搭」的开关，接反了就永远只会装那一个样例。
    #[test]
    fn resolve_prefers_the_one_built_for_this_customer() {
        let mine = broken(|w| {
            w["id"] = json!("lawyer");
            w["name"] = json!("合同律师工作台");
        });
        let got = resolve(Some("creator"), Some(&mine), &skills()).unwrap();
        assert_eq!(got["id"], json!("lawyer"), "给了现搭的就不该回落到样例");

        let fallback = resolve(None, None, &skills()).unwrap();
        assert_eq!(fallback["id"], json!("creator"), "没给才回落样例");
        // `null` 跟「没给」是一回事 —— JSON 里少写一个字段和写成 null 都很常见
        let null_inline = resolve(None, Some(&Value::Null), &skills()).unwrap();
        assert_eq!(null_inline["id"], json!("creator"));
    }

    /// ★ 可用性：入口在 ≠ 能力在。
    ///
    /// 「按客户情况现搭」那条路要靠 `uking-workbench` 技能；技能没装时**入口照样点得动**
    /// （起手词、新建项目那一问），AI 却不知道该怎么搭，大概率自己 mkdir 一通把闸门全绕过去。
    /// 所以 `ready` 必须跟着技能装没装走，而不是恒 true。
    ///
    /// 另一半同样重要：**没装技能时装内置样例仍然可用**（纯 Rust），blockers 里得说，
    /// 否则客户以为整个功能都废了。
    #[test]
    fn ready_follows_whether_the_skill_is_actually_installed() {
        let none: Vec<String> = Vec::new();
        let v = inspect(&ex(), None, false, &none);
        assert_eq!(v["ready"], json!(false), "一个 AI 都没装技能时不许说 ready");
        let b = v["blockers"].as_array().unwrap();
        assert_eq!(b.len(), 1, "说不清为什么不 ready 等于没说");
        let msg = b[0].as_str().unwrap();
        assert!(msg.contains("uking-workbench"), "得指名道姓缺哪个技能：{msg}");
        assert!(msg.contains("装内置样例"), "还得说清哪半边仍然能用，否则客户以为全废了：{msg}");
        assert_eq!(v["skill_installed_in"], json!([]), "报事实：一个都没有");

        let some = vec!["Claude Code".to_string(), "Hermes".to_string()];
        let v2 = inspect(&ex(), None, false, &some);
        assert_eq!(v2["ready"], json!(true));
        assert!(v2["blockers"].as_array().unwrap().is_empty(), "ready 了就不该还挂着 blocker");
        assert_eq!(v2["skill_installed_in"], json!(["Claude Code", "Hermes"]), "装在哪要如实列");
    }

    /// 盘点：只 stat 不读内容、报样本量、跳过隐藏目录和 node_modules。
    #[test]
    fn scan_counts_without_reading_anything() {
        let t = tmp("scan");
        std::fs::create_dir_all(t.join("子目录")).unwrap();
        std::fs::create_dir_all(t.join(".git")).unwrap();
        std::fs::create_dir_all(t.join("node_modules")).unwrap();
        std::fs::write(t.join("a.md"), "x").unwrap();
        std::fs::write(t.join("b.md"), "x").unwrap();
        std::fs::write(t.join("c.docx"), "x").unwrap();
        std::fs::write(t.join("子目录/d.md"), "x").unwrap();
        std::fs::write(t.join(".git/HEAD"), "x").unwrap();
        std::fs::write(t.join("node_modules/e.js"), "x").unwrap();

        let v = scan(&t).unwrap();
        assert_eq!(v["files"], json!(4), "隐藏目录和 node_modules 不该计入：{v}");
        assert_eq!(v["truncated"], json!(false));
        let md = v["by_ext"].as_array().unwrap().iter()
            .find(|e| e["name"] == json!(".md")).cloned().unwrap();
        assert_eq!(md["files"], json!(3));
        assert!(v["changed_in_30d"].as_u64().unwrap() >= 4, "刚写的文件必须算「最近动过」");
        // 不是目录要如实报错，不是返回一份空统计（空统计会被读成「这儿什么都没有」）
        assert!(scan(&t.join("a.md")).is_err());
        let _ = std::fs::remove_dir_all(&t);
    }

    /// 每个内嵌模板都得完整 —— 缺一段，客户机上生成的说明书就是残的，
    /// 而 AI 照残说明书干活的表现是「乱放文件」，很难被认成模板的问题。
    #[test]
    fn every_embedded_template_is_complete() {
        for id in ids() {
            let w = manifest(&id).expect("模板 JSON 必须能解析");
            for k in ["id", "name", "one_liner", "for_whom", "schema_version"] {
                assert!(!w[k].is_null(), "{id} 缺字段 {k}");
            }
            let dirs = w["dirs"].as_array().expect("dirs 必须是数组");
            assert!(!dirs.is_empty(), "{id} 一个目录都没有");
            for d in dirs {
                for k in ["path", "label", "purpose", "rule", "naming"] {
                    assert!(d[k].as_str().is_some_and(|s| !s.is_empty()), "{id} 目录缺 {k}: {d}");
                }
                let p = d["path"].as_str().unwrap();
                assert!(!p.contains("..") && !p.starts_with('/') && !p.contains(':'),
                    "{id} 目录 path 不许穿越/绝对路径: {p}");
            }
            // 动作声明要用的技能，必须真在这个模板的技能表里 —— 否则界面按钮点下去缺依赖
            let empty: Vec<Value> = Vec::new();
            let skills: Vec<&str> = w["skills"].as_array().unwrap_or(&empty)
                .iter().filter_map(|s| s["id"].as_str()).collect();
            for a in w["actions"].as_array().unwrap_or(&empty) {
                for n in a["needs"].as_array().unwrap_or(&empty) {
                    let n = n.as_str().unwrap_or("");
                    assert!(skills.contains(&n), "{id} 动作 {} 要 {n}，但技能表里没有", a["id"]);
                }
            }
            assert!(w["not_included"].as_array().is_some_and(|a| !a.is_empty()),
                "{id} 必须如实写「没有什么」");
        }
    }

    /// 闸门②：非空且不是我们建的 → 拒绝，**且一个字节都没写**。
    /// 这条挂了的后果是往客户桌面撒一堆文件夹。
    #[test]
    fn refuses_a_foreign_non_empty_folder_and_writes_nothing() {
        let t = tmp("foreign");
        std::fs::create_dir_all(&t).unwrap();
        std::fs::write(t.join("客户的稿子.docx"), "x").unwrap();

        let e = install(&ex(), &t, false).unwrap_err();
        assert!(e.contains(MARKER), "拒绝理由要说清判据: {e}");

        let left: Vec<_> = std::fs::read_dir(&t).unwrap().filter_map(|e| e.ok()).collect();
        assert_eq!(left.len(), 1, "被拒之后不许留下任何新东西");
        assert!(!t.join("WORKBENCH.md").exists());
        let _ = std::fs::remove_dir_all(&t);
    }

    /// 盘根/家目录就算是空的也不许 —— CLI/MCP/AI 都能传路径进来。
    #[test]
    fn refuses_drive_root_and_home() {
        let root = if cfg!(windows) { PathBuf::from("C:\\") } else { PathBuf::from("/") };
        assert!(install(&ex(), &root, false).is_err(), "盘根必须拒绝");
        let home = crate::installer::user_home_dir();
        assert!(install(&ex(), &home, false).is_err(), "家目录必须拒绝");
    }

    /// 装 → 再装。第二次一个都不新建，而且他改过的 README 一个字节不动。
    #[test]
    fn install_is_idempotent_and_never_clobbers_his_edits() {
        let t = tmp("idem");
        let r = install(&ex(), &t, false).unwrap();
        assert!(r["created"].as_u64().unwrap() > 0);
        assert!(t.join("WORKBENCH.md").exists() && t.join(MARKER).exists());

        // 客户改了一个目录说明
        let wb = manifest("creator").unwrap();
        let first = wb["dirs"][0]["path"].as_str().unwrap().to_string();
        let his = t.join(&first).join("README.md");
        std::fs::write(&his, "这是我自己写的，别动").unwrap();

        let r2 = install(&ex(), &t, false).unwrap();
        assert_eq!(r2["created"].as_u64().unwrap(), 0, "重跑不该新建任何东西");
        assert_eq!(r2["updated"].as_u64().unwrap(), 0, "没要求更新说明书就不许更新");
        assert_eq!(std::fs::read_to_string(&his).unwrap(), "这是我自己写的，别动");

        // 显式要求才更新说明书，且**只动我们生成的那几个**（说明书 + 契约 + 两个入口指针）
        let r3 = install(&ex(), &t, true).unwrap();
        assert_eq!(
            r3["updated"].as_u64().unwrap(),
            2 + ENTRYPOINTS.len() as u64,
            "只该更新 WORKBENCH.md + 契约 + 入口指针"
        );
        assert_eq!(std::fs::read_to_string(&his).unwrap(), "这是我自己写的，别动");
        let _ = std::fs::remove_dir_all(&t);
    }

    /// ★ 装完之后，AI **不用被人提醒**就能读到约定。
    ///
    /// 这条守的是 2026-08-11 补的那个洞：以前只落 `WORKBENCH.md`，而没有任何 CLI 会自动读
    /// 这个文件名 —— 客户搭完工作台，AI 进去照样一问三不知，那就是他反馈的「记忆混乱」。
    /// 判据不是「文件存在」，是**入口里真的把 AI 指向了全文**。
    #[test]
    fn entrypoints_exist_and_point_at_the_doc() {
        let t = tmp("entry");
        install(&ex(), &t, false).unwrap();
        let wb = manifest("creator").unwrap();

        for (name, _, other) in ENTRYPOINTS {
            let p = t.join(name);
            assert!(p.exists(), "{name} 没落地 —— {} 进来就读不到任何约定", name);
            let body = std::fs::read_to_string(&p).unwrap();
            assert!(body.contains("WORKBENCH.md"), "{name} 没把 AI 指向全文");
            assert!(body.contains(other), "{name} 该提一句另一份入口在哪");
            // 🔴 每个目录的**规矩本身**必须在入口里，不能只留一句「去读 WORKBENCH.md」。
            // 这条断言是被实测翻案过来的（2026-08-11）：纯指针版本里，Hermes 拿到了
            // AGENTS.md 却不去追全文，然后凭空断言「当前目录是空的」。
            // 指针保证得了 AI 拿到入口，保证不了它去读第二个文件 —— 所以最常问的事实要零跳转。
            for d in wb["dirs"].as_array().unwrap() {
                let path = d["path"].as_str().unwrap();
                assert!(body.contains(path), "{name} 漏了目录 {path}");
                let rule = d["rule"].as_str().unwrap();
                assert!(
                    body.contains(rule),
                    "{name} 里没有 `{path}` 的规矩 —— AI 不会为了它再去读 WORKBENCH.md（实测过）"
                );
            }
        }
        let _ = std::fs::remove_dir_all(&t);
    }

    /// 客户自己那份 `CLAUDE.md` 一个字节不动 —— 但**必须喊出来**。
    ///
    /// 默默跳过的后果不是「少个文件」，是这次「装好了」是假的：ok:true、created 一堆，
    /// 而 AI 进来读不到任何约定，客户回头说「你们这玩意还是不记事」。
    #[test]
    fn keeps_his_own_entrypoint_but_says_so_loudly() {
        let t = tmp("hisentry");
        install(&ex(), &t, false).unwrap();

        let his = t.join("CLAUDE.md");
        std::fs::write(&his, "# 我自己的规矩\n别动我").unwrap();

        let r = install(&ex(), &t, false).unwrap();
        assert_eq!(std::fs::read_to_string(&his).unwrap(), "# 我自己的规矩\n别动我", "不许覆盖他的");

        let warns = r["warnings"].as_array().expect("要有 warnings 字段");
        assert_eq!(warns.len(), 1, "正好一条：CLAUDE.md 被跳过了");
        assert!(warns[0].as_str().unwrap().contains("CLAUDE.md"), "警告要点名是哪个文件");
        assert!(r["next"].as_str().unwrap().contains("WORKBENCH.md"), "下一步要告诉他怎么补救");

        let _ = std::fs::remove_dir_all(&t);
    }

    /// 预览是只读的 —— 界面上点「看看会干什么」不该真在硬盘上留东西。
    #[test]
    fn inspect_previews_without_touching_disk() {
        let t = tmp("preview");
        let v = inspect(&ex(), Some(&t), false, &[]);
        assert_eq!(v["target"]["installable"], json!(true));
        assert!(v["target"]["plan"].as_array().unwrap().len() > 3);
        assert!(!t.exists(), "预览不许把目录建出来");

        // 说明书是从契约编译的：目录改了，说明书里必然跟着变
        let wb = manifest("creator").unwrap();
        let doc = compile_doc(&wb);
        for d in wb["dirs"].as_array().unwrap() {
            assert!(doc.contains(d["path"].as_str().unwrap()), "说明书漏了目录 {}", d["path"]);
        }
    }
}
