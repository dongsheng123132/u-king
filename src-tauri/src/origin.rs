//! 任务本象（`task.origin`）—— 2Origin 计算机架构里的**长期存储层**。
//!
//! # 它治什么病
//!
//! 「今天教会的，明天就忘；这个任务学的，下个会话从一年级重来。」
//! 根因是 agent 存的是**聊天记录（影子）**，不是**对象状态（本象）**。
//!
//! U-King 现在的 `tasks.json` 正是这个病的样本：36 条任务，字段是
//! `{id, name, dir, status, source, tool…}` —— 纯壳。它回答「哪个文件夹、叫什么、
//! 在不在跑」，**答不了「这个任务干到哪了、验过什么、下一步是什么」**。
//! 换个会话、换个 harness 接手，前面干的全白干。
//!
//! 本模块存的是后者，schema 对齐 `2origin-computer/schemas/task.origin.schema.json`。
//!
//! # 落点为什么不在用户的项目目录里
//!
//! 参考实现把 `task.origin.json` 放进工作目录（好处是别的 harness 的 SessionStart hook
//! 能自己捡到）。我们不这么干：**往客户的项目目录里塞文件是侵入**，而 U-King 本来就
//! 掌握着委派入口（`claude -p` / `codex exec` 的 env 与提示词），可以直接把状态喂进去。
//! 所以落在 `~/.uking/origin/<task_id>.json`，客户的目录一个字节都不动。
//!
//! # 三条不许破的规矩
//!
//! 1. **`facts` 里的 `verified` 只认机器复验**，不认 AI 自述。跟 metrics 那条
//!    「`verified` 只认机器复验」同一条纪律 —— 一个自称验过的事实，比没有更危险。
//! 2. **`version` 每次保存 +1**，且保存要带 `expected_version` 做乐观并发。
//!    两个 harness 同时写同一个任务时，后写的不许静默覆盖（宪法第 16 条）。
//! 3. **编译进上下文的内容必须有上限**。状态是要被塞进每一轮提示词的，
//!    无上限增长会让它自己变成账单。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 协议版本，冻结。schema 里是 const，改它等于换协议。
pub const SPEC: &str = "2origin/0.1";
pub const KIND: &str = "task.origin";

/// 编译进上下文时各段的条数上限 —— 见模块头第 3 条。
const MAX_FACTS_IN_CONTEXT: usize = 12;
const MAX_STEPS_IN_CONTEXT: usize = 8;
const MAX_DECISIONS_IN_CONTEXT: usize = 6;
/// 单条文本进上下文的字符上限（中文按字符切，别切出半个字）。
const MAX_LINE_CHARS: usize = 200;

/// 一条验证过的事实。**`verified` 是承诺，不是修辞。**
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Fact {
    pub claim: String,
    /// 只有机器复验过才是 true。AI 说「我验过了」不算。
    #[serde(default)]
    pub verified: bool,
    /// 这条事实是怎么来的（命令 / 文件 / 动作 id）。空 = 说不清出处，
    /// 那它就只是个说法，不该被下游当依据。
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub when: String,
    /// 作用域 —— **决定这条事实跨机之后还算不算数**（taskpack-0.1 §4.2）。
    ///
    /// | 取值 | 是什么 | 跨机 |
    /// |---|---|---|
    /// | `universal` | 判断、共识、客户要求、决策理由 | 保留 ✓ |
    /// | `org` | 一个组织内部的约定 | 保留 ✓，标注来源 |
    /// | `machine` | 路径、版本、装没装、端口、代理、「它能跑」 | **打包时封存为未证** |
    ///
    /// 🔴 **空 = 按 `machine` 处理**（规范原文：写不出别人怎么复验它，它就是 machine）。
    /// 这个默认方向是故意的：漏标只会让事实被多验一次，反过来会让「只在我这台机器成立」
    /// 的东西戴着 ✓ 跨机落地 —— 那正是这套设计要防的那件事。
    ///
    /// 本字段只**承载**，不执行降级：封存发生在打包那一端（`task-passport` 的 pack），
    /// 不在落地端。安全属性长在文件里、不长在接收方身上，接收方忘了降级也假不了 ✓。
    /// 加它之前 `Fact` 没有这个字段，于是走 U-King 这个 provider 的护照里
    /// `verified_on` 永远为 0 —— 机制在，但接不上。
    #[serde(default)]
    pub scope: String,
    /// 跨机后是否需要重验。打包端封存机器级事实时置 true。
    #[serde(default)]
    pub needs_reverify: bool,
    /// 它**曾经**被证明的那台机器。记「在哪」才分得开两件事：
    /// 「换了台机器」和「同一台机器，只是此刻够不着」。
    #[serde(default)]
    pub verified_on: String,
    /// 本结构没声明的字段**原样留着**。
    ///
    /// 🔴 根因从来不是 scope 特殊，而是**存储层静默丢弃它认不得的字段**：
    /// 同样被吞的还有 taskpack 的 `id`，以及后续要加的 `source_attachment`。
    /// 补一个 scope 只堵住了这次撞见的那一个洞。存储层不该替协议决定哪些字段
    /// 有意义 —— 它的职责是原样存回去。
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Decision {
    pub what: String,
    /// **为什么**比决定本身值钱：换个 harness 接手时，没有理由就只能重新纠结一遍。
    #[serde(default)]
    pub why: String,
    #[serde(default)]
    pub when: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ActionRec {
    pub verb: String,
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub result: String,
    #[serde(default)]
    pub evidence: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Learning {
    pub lesson: String,
    #[serde(default)]
    pub confidence: String,
    #[serde(default)]
    pub status: String,
}

/// 一个任务的本象。字段与 `task.origin.schema.json` 一一对应。
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TaskOrigin {
    pub spec: String,
    pub kind: String,
    pub id: String,
    #[serde(default)]
    pub title: String,
    /// 一句话目标。**换脑也能看懂**才算合格。
    pub goal: String,
    /// 每次保存 +1，乐观并发用。
    pub version: u64,
    #[serde(default)]
    pub machine_id: String,
    /// 上次写它的 harness（claude-code / codex / hermes…），兼容性标记。
    #[serde(default)]
    pub harness: String,
    #[serde(default)]
    pub scope: String,
    #[serde(default)]
    pub created_at: String,
    pub updated_at: String,
    /// **写「世界此刻是什么样」，不写聊天过程。**
    pub current_state: String,
    #[serde(default)]
    pub facts: Vec<Fact>,
    #[serde(default)]
    pub decisions: Vec<Decision>,
    #[serde(default)]
    pub actions: Vec<ActionRec>,
    #[serde(default)]
    pub artifacts: Vec<String>,
    /// 验证到什么程度、还差什么。**「还差什么」那半才是这个字段的价值。**
    #[serde(default)]
    pub verification: String,
    /// 换模型后靠它接着干。空 = 这个状态没法交接。
    pub next_steps: Vec<String>,
    #[serde(default)]
    pub learnings: Vec<Learning>,
}

impl TaskOrigin {
    /// 新建一个本象。`goal` 必填 —— 没有目标的状态交接不出去。
    pub fn new(id: &str, title: &str, goal: &str) -> Self {
        let now = now_iso();
        Self {
            spec: SPEC.into(),
            kind: KIND.into(),
            id: id.into(),
            title: title.into(),
            goal: goal.into(),
            version: 1,
            machine_id: String::new(),
            harness: String::new(),
            scope: String::new(),
            created_at: now.clone(),
            updated_at: now,
            current_state: String::new(),
            facts: Vec::new(),
            decisions: Vec::new(),
            actions: Vec::new(),
            artifacts: Vec::new(),
            verification: String::new(),
            next_steps: Vec::new(),
            learnings: Vec::new(),
        }
    }

    /// schema 的 required 字段齐不齐。**不齐就别交接** —— 一个缺目标或缺下一步的
    /// 状态，接手方拿到手还是得回头问人，等于没交接。
    pub fn missing_required(&self) -> Vec<&'static str> {
        let mut m = Vec::new();
        if self.spec != SPEC {
            m.push("spec");
        }
        if self.kind != KIND {
            m.push("kind");
        }
        if self.id.trim().is_empty() {
            m.push("id");
        }
        if self.goal.trim().is_empty() {
            m.push("goal");
        }
        if self.version == 0 {
            m.push("version");
        }
        if self.updated_at.trim().is_empty() {
            m.push("updated_at");
        }
        if self.current_state.trim().is_empty() {
            m.push("current_state");
        }
        if self.next_steps.is_empty() {
            m.push("next_steps");
        }
        m
    }

    /// ★ **北桥：把状态编译成塞进提示词的那一段。**
    ///
    /// 这是整个架构的关键一步 —— 状态存下来没用，**得让接手的模型看得懂、且不用回头问人**。
    /// 所以：目标和下一步永远在；事实按「验过的优先」排；每段都有上限。
    ///
    /// 刻意**不塞** `actions` 的全量记录：那是给审计用的，对接手方是噪声。
    pub fn compile_context(&self) -> String {
        let mut s = String::new();
        s.push_str("## 这个任务的当前状态（2Origin task.origin）\n\n");
        s.push_str(&format!("**目标**：{}\n", clip(&self.goal)));
        if !self.current_state.trim().is_empty() {
            s.push_str(&format!("**世界此刻**：{}\n", clip(&self.current_state)));
        }
        if !self.verification.trim().is_empty() {
            s.push_str(&format!("**验证到哪了**：{}\n", clip(&self.verification)));
        }

        // 事实：验过的排前面。未验证的**明着标出来**，别让接手方拿它当依据。
        if !self.facts.is_empty() {
            let mut fs: Vec<&Fact> = self.facts.iter().collect();
            fs.sort_by_key(|f| !f.verified);
            s.push_str("\n**已知事实**（✓=机器复验过，?=只是说法）：\n");
            for f in fs.iter().take(MAX_FACTS_IN_CONTEXT) {
                let mark = if f.verified { "✓" } else { "?" };
                let src = if f.source.trim().is_empty() {
                    String::new()
                } else {
                    format!("（出处：{}）", clip_short(&f.source))
                };
                s.push_str(&format!("- {mark} {}{src}\n", clip(&f.claim)));
            }
            if self.facts.len() > MAX_FACTS_IN_CONTEXT {
                s.push_str(&format!("- …另有 {} 条未列出\n", self.facts.len() - MAX_FACTS_IN_CONTEXT));
            }
        }

        // 决策：**带上为什么**。没有理由，接手方只会把同一个坑再纠结一遍。
        if !self.decisions.is_empty() {
            s.push_str("\n**已定的事**（含理由，别重新纠结）：\n");
            for d in self.decisions.iter().take(MAX_DECISIONS_IN_CONTEXT) {
                let why = if d.why.trim().is_empty() {
                    String::new()
                } else {
                    format!(" —— {}", clip(&d.why))
                };
                s.push_str(&format!("- {}{why}\n", clip(&d.what)));
            }
        }

        if !self.artifacts.is_empty() {
            s.push_str("\n**已产出**：");
            s.push_str(&self.artifacts.iter().take(8).map(|a| clip_short(a)).collect::<Vec<_>>().join(" · "));
            s.push('\n');
        }

        // 下一步永远在，且永远在最后 —— 接手方最需要的就是这一段。
        s.push_str("\n**下一步**：\n");
        if self.next_steps.is_empty() {
            s.push_str("- （没写下一步。这个状态交接不完整，先问人再动手。）\n");
        } else {
            for n in self.next_steps.iter().take(MAX_STEPS_IN_CONTEXT) {
                s.push_str(&format!("- {}\n", clip(n)));
            }
        }
        s
    }
}

/// 按字符截断（中文安全），并把换行压平 —— 提示词里一条就是一行。
fn clip(s: &str) -> String {
    let flat: String = s.chars().map(|c| if c == '\n' || c == '\r' { ' ' } else { c }).collect();
    let t = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.chars().count() <= MAX_LINE_CHARS {
        return t;
    }
    t.chars().take(MAX_LINE_CHARS).collect::<String>() + "…"
}

fn clip_short(s: &str) -> String {
    let t = s.trim();
    if t.chars().count() <= 60 {
        return t.to_string();
    }
    t.chars().take(60).collect::<String>() + "…"
}

/// UTC ISO8601。跟 `aitasks.rs` 一样自己算 —— 项目没有 chrono（体积优先）。
fn now_iso() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let tod = secs.rem_euclid(86_400);
    // civil_from_days（Howard Hinnant）
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        tod / 3600,
        (tod % 3600) / 60,
        tod % 60
    )
}

// ———————————————— 落盘 ————————————————

/// 本象目录。**不进客户的项目目录** —— 理由见模块头。
pub fn origin_dir() -> PathBuf {
    crate::installer::user_home_dir().join(".uking").join("origin")
}

fn path_of(task_id: &str) -> PathBuf {
    // 任务 id 是我们自己生成的（`sess-…` / `t1q7hgpc`），但仍然挡一手路径穿越：
    // 这个函数将来会被 AI 通过动作调用，入参不可信。
    let safe: String = task_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(120)
        .collect();
    origin_dir().join(format!("{safe}.json"))
}

pub fn load(task_id: &str) -> Option<TaskOrigin> {
    let text = std::fs::read_to_string(path_of(task_id)).ok()?;
    serde_json::from_str(&text).ok()
}

/// 保存，带乐观并发。
///
/// `expected_version` 给了就必须对得上 —— 对不上说明**在你读之后有别的 harness 写过**，
/// 直接拒绝，绝不覆盖（宪法第 16 条：「最后写入者获胜」不许当未声明的默认）。
/// 这不是理论风险：跨 harness 交接本来就是两个进程轮流写同一个状态。
pub fn save(mut o: TaskOrigin, expected_version: Option<u64>) -> Result<TaskOrigin, String> {
    let missing = o.missing_required();
    if !missing.is_empty() {
        return Err(format!("状态不完整，缺字段: {} —— 缺目标或缺下一步的状态交接不出去", missing.join(", ")));
    }
    if let Some(exp) = expected_version {
        let cur = load(&o.id).map(|x| x.version).unwrap_or(0);
        if cur != exp {
            return Err(format!("conflict: 磁盘上是 v{cur}，你基于 v{exp} 在写 —— 先读回来再改，别覆盖"));
        }
    }
    let dir = origin_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("建目录失败: {e}"))?;
    o.version = o.version.saturating_add(1);
    o.updated_at = now_iso();
    let body = serde_json::to_string_pretty(&o).map_err(|e| e.to_string())?;
    // 原子写：先写临时文件再 rename，避免半截文件（宪法第 10 条）。
    let tmp = path_of(&o.id).with_extension("json.tmp");
    std::fs::write(&tmp, body).map_err(|e| format!("写入失败: {e}"))?;
    std::fs::rename(&tmp, path_of(&o.id)).map_err(|e| format!("落盘失败: {e}"))?;
    Ok(o)
}

/// 这台机器上有本象的任务清单（只读，给看板/动作用）。
pub fn list() -> Vec<TaskOrigin> {
    let Ok(rd) = std::fs::read_dir(origin_dir()) else {
        return Vec::new();
    };
    let mut out: Vec<TaskOrigin> = rd
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .filter_map(|t| serde_json::from_str::<TaskOrigin>(&t).ok())
        .collect();
    out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-08-16 的真实事故：一本护照里 13 条事实在上游明确标了 `scope`，
    /// 存进来再读出去**全部消失**，打包端只能按「无 scope 即 machine」全员降级 ——
    /// 于是接手方拿到的包里，没有一条可用的已验证事实。
    ///
    /// 根因不是 scope 特殊，是存储层把认不得的字段静默吃掉了。所以这条测试同时钉两件事：
    /// 声明过的字段要活着，**没声明过的也要活着**。
    #[test]
    fn round_trip_keeps_scope_and_unknown_fields() {
        let raw = r#"{
            "claim": "体验课时间为 2026-08-22 20:00-21:30",
            "verified": true,
            "source": "01-文案终稿",
            "when": "",
            "scope": "universal",
            "needs_reverify": false,
            "verified_on": "zjzhfs",
            "id": "F2",
            "source_attachment": "01-文案.txt"
        }"#;

        let f: Fact = serde_json::from_str(raw).expect("Fact 必须能从 taskpack 的事实反序列化");
        assert_eq!(f.scope, "universal", "scope 丢了，跨机时这条就只能一刀切降级");
        assert_eq!(f.verified_on, "zjzhfs", "verified_on 分得开「换了机器」和「够不着」");
        assert!(!f.needs_reverify);

        let back = serde_json::to_value(&f).expect("Fact 必须能序列化回去");
        assert_eq!(back["scope"], "universal", "存一趟就把 scope 洗掉 = 协议的核心规则失效");
        // 存储层没声明过的字段同样不许吞：taskpack 的 id、以及后面要加的 source_attachment
        assert_eq!(back["id"], "F2", "认不得的字段不该被存储层吃掉");
        assert_eq!(back["source_attachment"], "01-文案.txt", "同上：存储层不替协议决定字段是否有意义");
    }

    #[test]
    fn required_fields_gate_the_handoff() {
        let mut o = TaskOrigin::new("t1", "标题", "把合同里的甲方改掉");
        // 新建的状态缺 current_state 和 next_steps —— **这正是要拦的**：
        // 一个只有目标、没有「世界此刻」和「下一步」的状态，接手方拿到还是得问人。
        let m = o.missing_required();
        assert!(m.contains(&"current_state"), "{m:?}");
        assert!(m.contains(&"next_steps"), "{m:?}");

        o.current_state = "已定位到 3 处甲方名称，未改".into();
        o.next_steps = vec!["用 doc.edit 改这 3 处，改完 doc.read 回读验证".into()];
        assert!(o.missing_required().is_empty());
    }

    #[test]
    fn compiled_context_carries_what_a_new_harness_needs() {
        let mut o = TaskOrigin::new("t2", "标题", "把合同里的甲方改掉");
        o.current_state = "已定位 3 处，未改".into();
        o.next_steps = vec!["改完回读验证".into()];
        o.facts = vec![
            Fact { claim: "未验证的猜测".into(), verified: false, source: String::new(), ..Default::default() },
            Fact { claim: "甲方在第 2/7/9 段".into(), verified: true, source: "doc.read".into(), scope: "machine".into(), ..Default::default() },
        ];
        o.decisions = vec![Decision { what: "写到新文件不就地改".into(), why: "原件是客户唯一底稿".into(), when: String::new() }];
        let c = o.compile_context();

        // 接手方最需要的三样：目标、世界此刻、下一步
        assert!(c.contains("把合同里的甲方改掉"));
        assert!(c.contains("已定位 3 处"));
        assert!(c.contains("改完回读验证"));
        // 验过的事实排在未验证的前面，且两者可区分
        let vi = c.find("甲方在第 2/7/9 段").unwrap();
        let ui = c.find("未验证的猜测").unwrap();
        assert!(vi < ui, "验过的事实必须排在前面");
        assert!(c.contains("✓ 甲方在第 2/7/9 段"));
        assert!(c.contains("? 未验证的猜测"), "未验证的必须明着标，别让接手方当依据");
        // 决策必须带理由
        assert!(c.contains("原件是客户唯一底稿"));
    }

    #[test]
    fn empty_next_steps_says_so_instead_of_pretending() {
        let mut o = TaskOrigin::new("t3", "", "目标");
        o.current_state = "x".into();
        // 绕过 save 的闸直接编译 —— 编译器本身也不许粉饰
        let c = o.compile_context();
        assert!(c.contains("交接不完整"), "没有下一步时必须说出来，不能只留一个空标题");
    }

    #[test]
    fn context_is_bounded() {
        let mut o = TaskOrigin::new("t4", "", "目标");
        o.current_state = "x".into();
        o.next_steps = (0..50).map(|i| format!("步骤{i}")).collect();
        o.facts = (0..50)
            .map(|i| Fact { claim: format!("事实{i}"), verified: true, source: String::new(), ..Default::default() })
            .collect();
        let c = o.compile_context();
        // 状态要被塞进每一轮提示词，无上限增长会让它自己变成账单
        assert!(!c.contains("步骤20"), "next_steps 没截断");
        assert!(c.contains("另有 38 条未列出"), "facts 截断了就得说，不能装作全给了");
        // 单条也要截
        let mut o2 = TaskOrigin::new("t5", "", "目标");
        o2.current_state = "长".repeat(500);
        o2.next_steps = vec!["s".into()];
        assert!(o2.compile_context().chars().count() < 1500);
    }

    #[test]
    fn save_refuses_to_overwrite_a_newer_state() {
        let sb = crate::testsandbox::enter("origin-concurrency", &[]);
        let _ = sb;
        let mut o = TaskOrigin::new("t6", "标题", "目标");
        o.current_state = "初始".into();
        o.next_steps = vec!["下一步".into()];
        let saved = save(o.clone(), None).expect("首次保存");
        assert_eq!(saved.version, 2, "保存一次应 +1");

        // harness A 基于 v2 改
        let mut a = saved.clone();
        a.current_state = "A 改的".into();
        // harness B 抢先基于 v2 写成功了
        let mut b = saved.clone();
        b.current_state = "B 改的".into();
        save(b, Some(2)).expect("B 基于 v2 写，应成功");

        // A 再基于 v2 写 —— 磁盘已经是 v3，必须拒
        let err = save(a, Some(2)).unwrap_err();
        assert!(err.starts_with("conflict"), "跨 harness 并发写必须拒绝覆盖，实际: {err}");
        assert_eq!(load("t6").unwrap().current_state, "B 改的", "被拒的那次不许留下痕迹");
    }

    #[test]
    fn incomplete_state_cannot_be_saved() {
        let sb = crate::testsandbox::enter("origin-incomplete", &[]);
        let _ = sb;
        let o = TaskOrigin::new("t7", "标题", "目标"); // 缺 current_state / next_steps
        let err = save(o, None).unwrap_err();
        assert!(err.contains("缺字段"), "{err}");
        assert!(load("t7").is_none(), "不完整的状态不许落盘");
    }

    #[test]
    fn iso_time_is_wellformed() {
        let t = now_iso();
        assert_eq!(t.len(), 20, "{t}");
        assert!(t.ends_with('Z') && t.contains('T'), "{t}");
        assert!(t.starts_with("202"), "{t}");
    }

    /// `scope` 必须原样穿过存取 —— 它是跨机降级（taskpack-0.1 §4.2）唯一的依据。
    ///
    /// 🔴 这条守的是一个**沉默**的失败：加这个字段之前 `Fact` 根本没有 scope，
    /// 打包端拿不到判据，于是所有事实都当 machine 处理、`verified_on` 永远为 0 ——
    /// 机制在，但接不上，而且**没有任何报错**（Bugscope A4：缺席不会自己发声）。
    /// 所以判据不能只看「字段存在」，要看它**存进去再读出来还是同一个值**。
    #[test]
    fn fact_scope_survives_a_round_trip() {
        let sb = crate::testsandbox::enter("origin-scope", &[]);
        let _ = sb;
        let mut o = TaskOrigin::new("t-scope", "作用域", "验证 scope 能存能读");
        o.current_state = "刚写完".into();
        o.next_steps = vec!["读回来对一遍".into()];
        o.facts = vec![
            Fact {
                claim: "产品定位是装/配/用三件事".into(),
                verified: true,
                scope: "universal".into(),
                ..Default::default()
            },
            Fact {
                claim: "本机 cargo build 通过".into(),
                verified: true,
                scope: "machine".into(),
                ..Default::default()
            },
            // 故意不写 scope：规范说空 = 按 machine 处理，这里只验它**如实存成空**，
            // 不在本层擅自补默认值 —— 补了就等于替打包端做了判断。
            Fact { claim: "没标作用域的一条".into(), verified: true, ..Default::default() },
        ];
        save(o, None).expect("应当能存");

        let back = load("t-scope").expect("应当能读回来");
        let scopes: Vec<&str> = back.facts.iter().map(|f| f.scope.as_str()).collect();
        assert_eq!(
            scopes,
            vec!["universal", "machine", ""],
            "scope 必须原样穿过存取；空的要保持空，由打包端按 machine 处理"
        );
    }

    /// 老护照（没有 scope 字段）必须照常读得进来 —— `#[serde(default)]` 的意义就在这儿。
    /// 加字段而让存量数据读不出来，是比不加更坏的结果。
    #[test]
    fn passports_written_before_scope_existed_still_load() {
        let json = r#"{"claim":"老数据","verified":true,"source":"","when":""}"#;
        let f: Fact = serde_json::from_str(json).expect("缺 scope 的老 Fact 必须还能反序列化");
        assert_eq!(f.claim, "老数据");
        assert!(f.verified);
        assert_eq!(f.scope, "", "老数据没有 scope，读出来是空，等同 machine");
    }
}
