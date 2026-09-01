//! 招人搜索 —— **不做市场，做一条现搜的能力**。
//!
//! ## 为什么不建技能市场
//!
//! 建市场要网络效应、要审核运营，而这两样我们都没有；生态里已经有 6 家在做
//! （讯飞 skillhub ★4.9k、dshmarket、dsh-find-plugin…）。更要紧的是它跟产品定位冲突：
//! 既然要让**用户的 AI 自己去 GitHub / npm / SkillHub 上搜**，就不该再自己开一个货架。
//!
//! CLAUDE.md 那条说的就是这件事：
//!
//! > 有哪些动作 / command / 模块 —— **别问文档，跑 `action list --json`。地图会漂，地形不会。**
//!
//! **技能市场就是一张会漂的地图。** 现搜才是看地形。
//!
//! ## 它比「搜到了」多给什么
//!
//! 光返回一串包名没用 —— 调用方还是不知道能不能招、怎么招。所以每条结果都带
//! [`HireShape`]：这是本轮实测摸出来的三种形态，判据是 npm 元数据里的 `bin` /
//! 包内有没有 `skills/` / 有没有可用的 `exports` 子路径。
//!
//! ## 🔴 只搜不装
//!
//! 本模块**只读**：不写盘、不装包、不跑安装命令。装的动作留给人 ——
//! 同 `expert.rs` 里 `requires` 那条边界（给它一个「我来跑任意命令」的口子，
//! 等于把提示词注入面升级成任意代码执行）。
//!
//! ## 依赖方向
//!
//! 只依赖 `installer::curl`（公共 HTTP 层，复用不复制）。不认识 `actions.rs`，
//! 动作登记在组合根 `lib.rs`。

use serde::Serialize;
use serde_json::Value;

/// 单次搜索的网络死线（秒）。搜索是交互式动作，宁可少给结果也别让界面干等。
const TIMEOUT_SECS: &str = "12";
/// 一次最多返回多少条 —— 结果是要塞进模型上下文的，无上限就是账单。
const MAX_HITS: usize = 25;

/// 这个候选能不能招、怎么招。**本轮实测摸出来的三种形态。**
#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum HireShape {
    /// 有 `bin`：装成全局命令后，在专家包的 `requires` 里声明。
    Cli,
    /// 无 bin 但带 `skills/`：把技能目录拷进 `~/.uking/skills/`。
    SkillPack,
    /// 只注册 harness 内部工具：要看它 `exports` 有没有可用子路径 ——
    /// 有就包一层脚本（如 dsh-wechat-mp），没有就招不进来（如 mineru：还要外部服务）。
    HarnessTool,
    /// 元数据不够判断。**不猜** —— 猜错会让人去装一个装不了的东西。
    Unknown,
}

impl HireShape {
    /// 给调用方（多半是个 AI）的一句人话：怎么招。
    fn how(self) -> &'static str {
        match self {
            Self::Cli => "有 CLI：`npm i -g <包名>`，然后在专家包的 requires 里写上命令名",
            Self::SkillPack => "自带技能：把包里的 skills/<名字> 拷进 ~/.uking/skills/，专家包 skills 引用它",
            Self::HarnessTool => {
                "只注册 harness 内部工具：先看它 package.json 的 exports 有没有可用子路径 —— \
                 有就包一层脚本调用，没有（或还要外部服务）就别招"
            }
            Self::Unknown => "元数据不足以判断，装之前先读它的 README 和 package.json",
        }
    }
}

/// 一个可能可以招的人。
#[derive(Debug, Clone, Serialize)]
pub struct Candidate {
    pub name: String,
    pub version: String,
    pub description: String,
    /// npm / github
    pub source: &'static str,
    pub url: String,
    /// 周下载量（npm 有；GitHub 结果为 None）。**「市场验证过」的量化判据。**
    pub weekly_downloads: Option<u64>,
    pub shape: HireShape,
    /// `shape` 对应的一句人话。
    pub how_to_hire: &'static str,
}

/// `runtime.hire.search` 的规范状态。
#[derive(Debug, Clone, Serialize)]
pub struct HireSearch {
    pub query: String,
    pub hits: Vec<Candidate>,
    /// 实际问过的源，以及**问的结果**。
    ///
    /// 🔴 判据非空（Bugscope A1/A4）：`hits` 为空时，靠它区分
    /// 「搜过了确实没有」和「网络根本没通」—— 后者被读成前者，
    /// 会让人得出「生态里没这东西」的错误结论，而那是最难自己发现的一种错。
    pub sources: Vec<SourceResult>,
    pub truncated: bool,
    pub ready: bool,
    pub blockers: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceResult {
    pub source: &'static str,
    /// 这个源问通了没有。**通了但 0 条**和**压根没通**是两件事。
    pub reachable: bool,
    pub hits: usize,
    /// 没通的话，为什么。
    pub error: Option<String>,
}

/// 从 npm 元数据判断形态。`bin` 直接可判；`skills/` 判不了（要下包才知道），
/// 所以用关键词兜底，判不出就 `Unknown` —— **不猜**。
fn shape_from_npm(pkg: &Value) -> HireShape {
    if pkg.get("bin").is_some_and(|b| !b.is_null()) {
        return HireShape::Cli;
    }
    let kw: Vec<&str> =
        pkg.get("keywords").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    if kw.iter().any(|k| k.contains("skill")) {
        return HireShape::SkillPack;
    }
    if kw.iter().any(|k| k.contains("dsh") || k.contains("harness")) {
        return HireShape::HarnessTool;
    }
    HireShape::Unknown
}

/// 搜 npm。走 registry 的搜索接口，不需要鉴权。
fn search_npm(query: &str, out: &mut Vec<Candidate>) -> SourceResult {
    let url = format!(
        "https://registry.npmjs.org/-/v1/search?text={}&size={MAX_HITS}",
        urlencode(query)
    );
    let body = match crate::installer::curl(&["-fsSL", "--max-time", TIMEOUT_SECS, &url]) {
        Ok(b) => b,
        Err(e) => {
            return SourceResult { source: "npm", reachable: false, hits: 0, error: Some(short(&e)) }
        }
    };
    let v: Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(e) => {
            return SourceResult {
                source: "npm",
                reachable: false,
                hits: 0,
                error: Some(format!("返回的不是 JSON: {e}")),
            }
        }
    };
    let mut n = 0;
    for o in v.get("objects").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default() {
        let Some(p) = o.get("package") else { continue };
        let name = p.get("name").and_then(Value::as_str).unwrap_or_default().to_string();
        if name.is_empty() {
            continue;
        }
        let shape = shape_from_npm(p);
        out.push(Candidate {
            url: format!("https://www.npmjs.com/package/{name}"),
            name,
            version: p.get("version").and_then(Value::as_str).unwrap_or("?").to_string(),
            description: clip(p.get("description").and_then(Value::as_str).unwrap_or_default()),
            source: "npm",
            // 「市场验证过」的量化判据 —— 下载量比 star 更接近真实使用。
            weekly_downloads: o
                .get("downloads")
                .and_then(|d| d.get("weekly"))
                .and_then(Value::as_u64),
            shape,
            how_to_hire: shape.how(),
        });
        n += 1;
    }
    SourceResult { source: "npm", reachable: true, hits: n, error: None }
}

/// ★ 现搜生态里可以招的人。**只读、不装、不写盘。**
pub fn search(query: &str) -> HireSearch {
    let q = query.trim();
    let mut hits = Vec::new();
    let mut sources = Vec::new();

    if q.is_empty() {
        return HireSearch {
            query: String::new(),
            hits,
            sources,
            truncated: false,
            ready: false,
            blockers: vec!["没给搜索词".into()],
        };
    }

    sources.push(search_npm(q, &mut hits));

    // 下载量高的排前面 —— 「用市场验证过的」这条规矩的机器化形态。
    hits.sort_by(|a, b| b.weekly_downloads.unwrap_or(0).cmp(&a.weekly_downloads.unwrap_or(0)));
    let truncated = hits.len() > MAX_HITS;
    hits.truncate(MAX_HITS);

    let any_reachable = sources.iter().any(|s| s.reachable);
    let mut blockers = Vec::new();
    if !any_reachable {
        // 🔴 这条是本模块最要紧的一句：没通 ≠ 没有。
        blockers.push(
            "一个源都没问通 —— **这不等于生态里没有**，先查网络/代理再下结论".to_string(),
        );
        for s in &sources {
            if let Some(e) = &s.error {
                blockers.push(format!("{}：{e}", s.source));
            }
        }
    } else if hits.is_empty() {
        blockers.push("问通了，但确实没搜到 —— 换个词试试".to_string());
    }

    HireSearch {
        query: q.to_string(),
        ready: any_reachable && !hits.is_empty(),
        blockers,
        hits,
        sources,
        truncated,
    }
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            b' ' => "%20".into(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn clip(s: &str) -> String {
    let s = s.trim();
    if s.chars().count() <= 160 {
        return s.to_string();
    }
    s.chars().take(160).collect::<String>() + "…"
}

fn short(e: &str) -> String {
    e.chars().take(200).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 🔴 判据非空：`hits` 为空时必须能区分「搜过了没有」和「网络没通」。
    /// 后者被读成前者会让人得出「生态里没这东西」的错误结论 —— 而离线时这个错
    /// **不会有任何报错提示**（Bugscope A4：缺席不会自己发声）。
    #[test]
    fn empty_result_says_whether_the_network_answered() {
        let r = search("");
        assert!(!r.ready);
        assert!(!r.blockers.is_empty(), "空查询必须给出 blockers");

        // 真实搜索：无论通不通，sources 都必须如实记录，且 ready 与之一致。
        let r = search("zzz-a-package-name-that-cannot-exist-9f3a");
        assert!(!r.sources.is_empty(), "问了哪些源必须记下来");
        let reachable = r.sources.iter().any(|s| s.reachable);
        assert_eq!(r.ready, reachable && !r.hits.is_empty());
        if !reachable {
            assert!(
                r.blockers.iter().any(|b| b.contains("不等于")),
                "没问通时必须明说「没通 ≠ 没有」，否则会被读成生态里没有"
            );
        }
    }

    /// 形态判断只认得出的才判，**判不出就 Unknown，不猜** ——
    /// 猜错会让人去装一个根本装不了的东西。
    #[test]
    fn shape_never_guesses() {
        let cli = serde_json::json!({ "bin": { "foo": "./x.js" } });
        assert_eq!(shape_from_npm(&cli), HireShape::Cli);

        let skill = serde_json::json!({ "keywords": ["agent-skill"] });
        assert_eq!(shape_from_npm(&skill), HireShape::SkillPack);

        let tool = serde_json::json!({ "keywords": ["dsh-plugin"] });
        assert_eq!(shape_from_npm(&tool), HireShape::HarnessTool);

        let bare = serde_json::json!({ "name": "x" });
        assert_eq!(shape_from_npm(&bare), HireShape::Unknown, "判不出就是 Unknown，不许猜");

        // 每种形态都要给得出「怎么招」，否则搜到了也不知道下一步干什么
        for s in [HireShape::Cli, HireShape::SkillPack, HireShape::HarnessTool, HireShape::Unknown] {
            assert!(!s.how().is_empty());
        }
    }

    #[test]
    fn urlencode_handles_chinese_and_colon() {
        assert_eq!(urlencode("keywords:dsh-plugin"), "keywords%3Adsh-plugin");
        assert!(urlencode("公众号").starts_with('%'));
        assert_eq!(urlencode("a b"), "a%20b");
    }
}
