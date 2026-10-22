//! 「每轮底座账单」—— 只读扫描本机 skills 目录与记忆文件，量化「每轮对话要替用户背多少上下文」。
//!
//! ## 为什么要有它
//! 用户装完一堆技能包后，最直接的体感是「变慢、变贵」，但界面上没有任何数字能解释这件事：
//! 每轮真正发出去的并不只是那一句提问，还有**每轮都要重新塞进去的底座** —— 技能清单、
//! `CLAUDE.md` / `AGENTS.md` / `llms.txt`。这份账单把它们摊开：几个技能包、各多少字符、
//! 记忆文件多大，再给一个 Claude Code 底座 token 的量级参考。
//!
//! 它只回答「本机现在背着多少」，**不参与计费、不推断模型实际计费、也不改任何文件** ——
//! 纯只读扫描，扫不到就报 0，让前端永远拿得到一张完整的账单。
//!
//! ## 口径
//! - 技能目录三处：`<home>/.claude/skills`、`<home>/.claude/skills-off`、`<home>/.uking/skills`。
//!   `count` = 含 `SKILL.md` 的**一级子目录**数；`chars` = 这些 `SKILL.md` 的**字符数**合计
//!   （`chars().count()`，不是字节数：中文技能说明一个汉字 3 字节，按字节报会吓到人）。
//! - 记忆文件：存在才收；`bytes` 用文件字节数（原始文件里的中文按 UTF-8 计，跟磁盘占用一致）。
//! - 家目录**一律走公共层** [`crate::installer::user_home_dir`]（认 `UKING_TEST_HOME` 沙箱），
//!   不自己读 `USERPROFILE`/`HOME` —— 同一事实只留一份，沙箱测试才不会探到真实用户目录。
//!
//! ## 降级
//! 目录不存在、权限不足、文件坏编码、读到一半被删 —— 一律静默降级成 0/跳过，**不报错、不 panic**
//! （release 是 `panic = abort`，只读展示功能更不值得把整个命令炸掉）。
//!
//! 纯 std + serde，无网络、无缓存。

use serde::Serialize;
use std::path::Path;

/// Claude Code 的底座（system prompt + 内置工具说明）token 量级。
///
/// 2026-10-22 本机实测 Claude Code 2.1.302 约 20.7k。写死常量而不是每次去跑一遍 CLI：
/// 那是上游版本行为，真变了也只会让这个参考值慢慢过期，不影响下面三项本机扫描结果；
/// 前端只拿它做「量级参考」，不参与任何计费/判断。
pub const CLAUDE_BASE_TOKENS: u64 = 20_700;

/// 一个技能目录的统计。
#[derive(Debug, Clone, Serialize)]
pub struct SkillStat {
    /// 扫的是哪个目录（展示用；目录不存在也照原样回传，前端能看出「扫的是空的」）。
    pub dir: String,
    /// 含 `SKILL.md` 的一级子目录数。
    pub count: usize,
    /// 这些 `SKILL.md` 的字符数合计（按 Unicode 字符，不按字节）。
    pub chars: u64,
}

/// 一个「每轮都会塞进上下文」的记忆文件。
#[derive(Debug, Clone, Serialize)]
pub struct MemoryFile {
    /// 稳定键：`claude_md` / `agents_md` / `llms_txt`（前端按它选文案，不解析路径）。
    pub key: String,
    /// 绝对路径（展示用）。
    pub path: String,
    /// 文件字节数。
    pub bytes: u64,
}

/// 每轮底座账单（前端直接渲染）。
#[derive(Debug, Clone, Serialize)]
pub struct ContextBill {
    /// Claude Code 底座的 token 量级参考（常量，见 [`CLAUDE_BASE_TOKENS`]）。
    pub claude_base_tokens: u64,
    /// `<home>/.claude/skills`
    pub claude_skills: SkillStat,
    /// `<home>/.claude/skills-off`（被停用的技能包也占磁盘，账单里单列）
    pub claude_skills_off: SkillStat,
    /// `<home>/.uking/skills`（虾盘云下发的 uking-* 技能包落点）
    pub uking_skills: SkillStat,
    /// `<home>/.claude/CLAUDE.md`、`<home>/AGENTS.md`、`<home>/.uking/llms.txt`（存在才收）。
    pub memory_files: Vec<MemoryFile>,
}

/// 扫描本机「每轮底座账单」。无参数、只读、永不失败。
#[tauri::command]
pub fn context_bill() -> ContextBill {
    bill_for_home(&crate::installer::user_home_dir())
}

/// 拆出「给定家目录」的纯函数版本：测试直接喂沙箱 root，不必再经环境变量绕一层。
fn bill_for_home(home: &Path) -> ContextBill {
    ContextBill {
        claude_base_tokens: CLAUDE_BASE_TOKENS,
        claude_skills: scan_skills(&home.join(".claude").join("skills")),
        claude_skills_off: scan_skills(&home.join(".claude").join("skills-off")),
        uking_skills: scan_skills(&home.join(".uking").join("skills")),
        memory_files: memory_files(home),
    }
}

/// 统计一个技能根目录：`SKILL.md` 所在的一级子目录数与字符数。
///
/// 只认一级子目录（技能包规范就是「一个目录一份 `SKILL.md`」）；目录里散放的 `.md` 文件、
/// 更深层的嵌套目录都不算 —— 那不是任何工具会当技能加载的东西，计进去只会虚报账单。
fn scan_skills(dir: &Path) -> SkillStat {
    let mut count = 0usize;
    let mut chars = 0u64;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let skill_dir = entry.path();
            if !skill_dir.is_dir() {
                continue;
            }
            let skill_md = skill_dir.join("SKILL.md");
            if !skill_md.is_file() {
                continue;
            }
            count += 1;
            // 坏编码 / 读失败只丢这一份的字符数（用 lossy 不 panic）；技能数照记，
            // 让前端看到「有 3 个包但只统计到 2 个的大小」比整张账单消失更有用。
            if let Ok(bytes) = std::fs::read(&skill_md) {
                chars += String::from_utf8_lossy(&bytes).chars().count() as u64;
            }
        }
    }
    SkillStat { dir: path_string(dir), count, chars }
}

/// 收集存在的记忆文件，顺序固定（claude_md → agents_md → llms_txt），不保证展示顺序靠调用方。
fn memory_files(home: &Path) -> Vec<MemoryFile> {
    let candidates = [
        ("claude_md", home.join(".claude").join("CLAUDE.md")),
        ("agents_md", home.join("AGENTS.md")),
        ("llms_txt", home.join(".uking").join("llms.txt")),
    ];
    candidates
        .into_iter()
        .filter_map(|(key, path)| {
            let meta = std::fs::metadata(&path).ok()?;
            if !meta.is_file() {
                return None;
            }
            Some(MemoryFile { key: key.to_string(), path: path_string(&path), bytes: meta.len() })
        })
        .collect()
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 写测试夹具：自动补父目录，省得每个用例重复 `create_dir_all`。
    fn write(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().expect("夹具路径必须有父目录")).expect("建夹具目录失败");
        std::fs::write(path, content).expect("写夹具失败");
    }

    /// 全量口径：只数含 `SKILL.md` 的一级子目录；中文按字符计数不按字节；记忆文件按固定顺序收。
    #[test]
    fn counts_only_dirs_with_skill_md_and_sums_chars() {
        crate::testsandbox::with_sandbox("context-bill-full", &[], |root| {
            write(&root.join(".claude").join("skills").join("alpha").join("SKILL.md"), "你好abc");
            write(&root.join(".claude").join("skills").join("beta").join("SKILL.md"), "x");
            // 没有 SKILL.md 的子目录、以及一级散放的 .md 文件，都不算技能。
            std::fs::create_dir_all(root.join(".claude").join("skills").join("no-skill")).unwrap();
            write(&root.join(".claude").join("skills").join("notes.md"), "不是技能");
            // 更深一层嵌套也不数（只认一级子目录）。
            write(&root.join(".claude").join("skills").join("deep").join("inner").join("SKILL.md"), "nested");
            write(&root.join(".claude").join("skills-off").join("legacy").join("SKILL.md"), "旧");
            write(&root.join(".uking").join("skills").join("gamma").join("SKILL.md"), "abc");
            write(&root.join(".claude").join("CLAUDE.md"), "记忆");
            write(&root.join("AGENTS.md"), "agents");
            write(&root.join(".uking").join("llms.txt"), "llms");

            let bill = context_bill();

            assert_eq!(bill.claude_base_tokens, CLAUDE_BASE_TOKENS);
            assert_eq!(bill.claude_skills.count, 2, "只数 alpha/beta，不数 no-skill/notes.md/deep");
            assert_eq!(bill.claude_skills.chars, 6, "「你好abc」5 字符 + 「x」1 字符，按字符不按字节");
            assert_eq!(
                bill.claude_skills.dir,
                root.join(".claude").join("skills").display().to_string()
            );
            assert_eq!(bill.claude_skills_off.count, 1);
            assert_eq!(bill.claude_skills_off.chars, 1);
            assert_eq!(bill.uking_skills.count, 1);
            assert_eq!(bill.uking_skills.chars, 3);

            let keys: Vec<&str> = bill.memory_files.iter().map(|f| f.key.as_str()).collect();
            assert_eq!(keys, vec!["claude_md", "agents_md", "llms_txt"], "顺序固定，方便前端直排");
            assert_eq!(bill.memory_files[0].bytes, "记忆".len() as u64, "UTF-8 中文 3 字节/字");
            assert_eq!(
                bill.memory_files[0].path,
                root.join(".claude").join("CLAUDE.md").display().to_string()
            );
            assert_eq!(bill.memory_files[1].bytes, 6);
            assert_eq!(bill.memory_files[2].bytes, 4);
        });
    }

    /// 目录不存在 / 没建记忆文件 = 全 0 + 空数组，不报错（新机器首启就是这个状态）。
    #[test]
    fn missing_directories_degrade_to_zero_and_empty() {
        crate::testsandbox::with_sandbox("context-bill-empty", &[], |root| {
            let bill = context_bill();
            for stat in [&bill.claude_skills, &bill.claude_skills_off, &bill.uking_skills] {
                assert_eq!(stat.count, 0);
                assert_eq!(stat.chars, 0);
                assert!(!stat.dir.is_empty(), "目录没扫到也要回传路径，前端才知道扫的是哪");
            }
            assert!(bill.memory_files.is_empty(), "一个记忆文件都不存在时必须是空数组");
            // 沙箱 root 就是本次的家目录口径：路径必须以它开头，不能漏到真实家目录。
            assert!(
                bill.claude_skills.dir.starts_with(&root.display().to_string()),
                "必须扫在沙箱里，实际是 {}",
                bill.claude_skills.dir
            );
        });
    }

    /// 坏编码的 `SKILL.md` 不许 panic：lossy 后按替换字符计数，技能数照记。
    #[test]
    fn invalid_utf8_skill_md_is_lossy_not_panic() {
        crate::testsandbox::with_sandbox("context-bill-lossy", &[], |root| {
            let path = root.join(".claude").join("skills").join("broken").join("SKILL.md");
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, [0xFFu8, b'o', b'k']).unwrap();

            let bill = context_bill();

            assert_eq!(bill.claude_skills.count, 1);
            assert_eq!(bill.claude_skills.chars, 3, "坏字节变一个替换字符，加 ok 共 3");
        });
    }

    /// 锁 JSON 形状：前端读的是序列化后的键名，改结构名不许静默漂掉。
    #[test]
    fn json_fields_match_frontend_contract() {
        crate::testsandbox::with_sandbox("context-bill-json", &[], |_root| {
            let value = serde_json::to_value(context_bill()).expect("ContextBill 必须能序列化");
            assert_eq!(value["claude_base_tokens"], serde_json::json!(CLAUDE_BASE_TOKENS));
            for key in ["dir", "count", "chars"] {
                assert!(value["claude_skills"].get(key).is_some(), "claude_skills 缺字段 {key}");
            }
            assert!(value["memory_files"].is_array(), "memory_files 必须是数组");
            let file = serde_json::to_value(MemoryFile {
                key: "agents_md".into(),
                path: "C:/demo/AGENTS.md".into(),
                bytes: 7,
            })
            .expect("MemoryFile 必须能序列化");
            assert_eq!(file["key"], "agents_md");
            assert_eq!(file["bytes"], 7);
        });
    }
}
