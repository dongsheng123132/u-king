//! 虾盘云模型目录 —— 「有哪些模型、各自收不收图」的**唯一真相源**，可热下发。
//!
//! ## 为什么要有它（2026-10-03 客户机 + 逐个实测）
//! 模型 id 硬编码在三十多个文件里，「哪个模型能收图」又单独散在好几处，结果已经漂了：
//! - DSH 配置漏声明 `input:["text","image"]`，pi-ai 适配层对没写 `input` 的自定义模型一律当纯文本，
//!   带图直接本地拒绝 —— 客户机实测「DSH + 虾盘云发不了图」，请求根本没到服务器；
//! - 看图脚本把 deepseek-chat / deepseek-flash 标成纯文本，实测它们能读图；
//! - 「换模型」下拉里挂着上游已经下线的 `gemini-3.5-flash`（调用返回 "no longer available"）。
//! 改一个模型要改三十处，必然漏。所以：**一份 JSON，所有人读它**。
//!
//! ## 两份文件，一条下发通道
//! - `models/xiapan-models.json`：`include_str!` 编进 exe 的兜底（离线 / 网站没上线时用它）；
//! - `website/skills/xiapan-models.json`：部署到服务器，跟 `install-windows.json` 同一套部署路径和镜像顺序。
//!
//! 覆盖规则跟装机清单一致：**线上（或本地缓存）的 `version` 严格大于内嵌的才采用**。
//! 闸门 `scripts/check-model-catalog-sync.mjs` 保证两份除 `version` 外一致、线上 ≥ 内嵌。
//!
//! ## 取用顺序
//! 内嵌 → 本地缓存 `~/.uking/cache/xiapan-models.json`（version 更大才认）→ 后台拉到的线上副本
//! （version 更大才认，并写回缓存）。**任何一环出问题都静默退回上一环**：坏 JSON、缺字段、
//! default/strong 指向不存在的模型、`input` 里出现 text/image 以外的东西 —— 一律整份丢弃，
//! 绝不让一份坏文件把「换模型」和「能不能发图」整个掐死。
//!
//! ## 目录里有谁（2026-10-03，全部在虾盘云 `/v1/chat/completions` 用同一张含 58273 的测试图逐个实测）
//! 8 个对话模型 + 2 个作图模型。换模型 / 上下线某个模型，只要改线上的 `xiapan-models.json`（抬高 `version`），
//! 不用发版。作图清单（`image_models`）是独立字段，跟对话清单、[`accepts_image`] 互不参与。
//!
//! ## 只认明确写了 image 的
//! [`accepts_image`] 只在目录里**明确**写了 `image` 才返回 true；不认识的 id 一律 false。
//! 宁可不声明，不许谎报能收图 —— 谎报的后果是模型收下图片却编一个答案（见 vision 技能的闸门）。
//!
//! 纯 std + serde_json；HTTP 走 `installer::curl`（系统 curl.exe + CREATE_NO_WINDOW）。

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// 内嵌兜底。解析失败是**发版前**就该被 `embedded_json_is_valid` 抓到的事，不是运行时的事。
const EMBEDDED: &str = include_str!("../models/xiapan-models.json");

/// 目录文件自报的名字；对不上就不是这份文件（防止把别的 JSON 误当目录吞进来）。
const CATALOG_NAME: &str = "xiapan-models";

/// 线上副本，依次尝试，第一个「合法且更新」的生效。顺序和 `installer.rs::SKILL_URLS` 保持一致
/// （u-claw.org.cn 放第一；闸门会核对两边的域名序列没漂）。
/// 2026-10-04 删掉 www.u-king.org（HTTPS 握手失败）与 u-king-org.vercel.app（DEPLOYMENT_NOT_FOUND，
/// 部署已不存在、名字可能被他人注册）两个回落源 —— 官网只认已备案的 u-claw.org.cn。
const CATALOG_URLS: &[&str] = &[
    // u-claw.org.cn 是唯一全国内可达子域（cloud.u-claw.org 部分网络 GFW SNI reset）
    "https://u-claw.org.cn/uking/xiapan-models.json",
    "https://cloud.u-claw.org/uking/xiapan-models.json",
];

/// 单次拉取上限：目录只有几 KB，超过这个量级的响应一定不是它（也防被劫持成大文件）。
const MAX_BYTES: usize = 256 * 1024;
/// 启动后延迟多久再拉：让首屏先稳住，跟技能包同步 / 静默升级那几个后台活错开。
const START_DELAY: Duration = Duration::from_secs(7);
/// 整轮刷新的预算。每条镜像 curl 自己有 6s 超时，但失败时 `installer::curl` 还会回退 .NET 再试一次，
/// 四条串起来最坏要好几分钟 —— 预算用尽就不再起新的一条，后台线程必然有限时退出。
const REFRESH_BUDGET: Duration = Duration::from_secs(45);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub desc: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub recommend: bool,
    /// 输入模态：`["text"]` 或 `["text","image"]`。**必填** —— 缺了就整份丢弃，
    /// 不替线上文件的笔误做「默认纯文本」的猜测。
    pub input: Vec<String>,
    /// 最后一次用真请求核实这条的日期。
    #[serde(default)]
    pub verified: String,
}

/// 作图模型（跟对话清单分开：作图走 `/v1/images/*`，「收不收图」这个问题对它没有意义）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImageModel {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub desc: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub recommend: bool,
    /// 支不支持「图生图 / 改图」（`/v1/images/edits`）。**必填、没有默认值**：没实测过的不许默认成 true，
    /// 否则客户挂上参考图选中它，收到的是一句看不懂的上游报错。
    pub edits: bool,
    /// 最后一次真请求核实的日期；没核实过就留空，不许编。
    #[serde(default)]
    pub verified: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Group {
    pub group: String,
    /// `true` = 这一组是「更聪明、更费额度」的贵档：U-Chat 的模型菜单把它折进「更多」，别让人随手点到。
    /// 以前靠组名里带不带「全球旗舰」四个字来认，改个组名就悄悄失效；现在显式写在数据里。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pricey: bool,
    pub items: Vec<Item>,
}

/// 老客户端读到带新字段的线上目录时，serde 默认忽略未知字段，所以往里加字段对它们是安全的。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Catalog {
    pub catalog: String,
    pub version: u64,
    #[serde(default)]
    pub updated: String,
    /// 开箱即用的默认模型（= 虾盘云 preset 的默认）。
    pub default: String,
    /// 「满血」档（难题再切的那个）。
    pub strong: String,
    pub groups: Vec<Group>,
    /// 作图模型清单。老版本的线上文件可能没有这个字段，缺省为空。
    #[serde(default)]
    pub image_models: Vec<ImageModel>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Embedded,
    Cache,
    Online,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Embedded => "embedded",
            Source::Cache => "cache",
            Source::Online => "online",
        }
    }
}

/// 当前生效的目录 + 它是从哪来的。
#[derive(Debug, Clone)]
pub struct Effective {
    pub source: Source,
    pub catalog: Arc<Catalog>,
}

// ============================================================
// 校验 / 解析（纯函数，单测直接打）
// ============================================================

/// 整份目录的合法性。任何一条不满足都返回 Err —— 调用方整份丢弃、退回上一环。
fn validate(c: &Catalog) -> Result<(), String> {
    if c.catalog != CATALOG_NAME {
        return Err(format!("catalog 字段应为 \"{CATALOG_NAME}\"，实际是 \"{}\"", c.catalog));
    }
    if c.version == 0 {
        return Err("version 必须 ≥ 1".into());
    }
    if c.groups.is_empty() {
        return Err("groups 为空".into());
    }
    let mut seen = std::collections::HashSet::new();
    for g in &c.groups {
        if g.group.trim().is_empty() {
            return Err("存在空名字的分组".into());
        }
        if g.items.is_empty() {
            return Err(format!("分组「{}」没有任何条目", g.group));
        }
        for it in &g.items {
            let id = it.id.as_str();
            if id.is_empty() || id != id.trim() || id.chars().any(|ch| ch.is_whitespace() || ch.is_control()) {
                return Err(format!("非法的模型 id {id:?}（空、带空白或控制字符）"));
            }
            if it.label.trim().is_empty() {
                return Err(format!("{id} 缺 label"));
            }
            if !seen.insert(id.to_string()) {
                return Err(format!("模型 id 重复：{id}"));
            }
            if it.input.is_empty() {
                return Err(format!("{id} 的 input 为空"));
            }
            for m in &it.input {
                if m != "text" && m != "image" {
                    return Err(format!("{id} 的 input 含不认识的模态 {m:?}（只许 text / image）"));
                }
            }
        }
    }
    for (what, id) in [("default", &c.default), ("strong", &c.strong)] {
        if !seen.contains(id.as_str()) {
            return Err(format!("{what} 指向的模型 {id:?} 不在条目里"));
        }
    }
    let mut seen_img = std::collections::HashSet::new();
    for m in &c.image_models {
        let id = m.id.as_str();
        if id.is_empty() || id != id.trim() || id.chars().any(|ch| ch.is_whitespace() || ch.is_control()) {
            return Err(format!("非法的作图模型 id {id:?}（空、带空白或控制字符）"));
        }
        if m.label.trim().is_empty() {
            return Err(format!("作图模型 {id} 缺 label"));
        }
        if !seen_img.insert(id.to_string()) {
            return Err(format!("作图模型 id 重复：{id}"));
        }
    }
    Ok(())
}

fn parse_validated(text: &str) -> Result<Catalog, String> {
    let c: Catalog = serde_json::from_str(text).map_err(|e| format!("JSON 解析失败：{e}"))?;
    validate(&c)?;
    Ok(c)
}

/// 候选目录是否值得采用：合法，且 `version` **严格大于** `baseline`。
fn adopt_candidate(text: &str, baseline: u64) -> Result<Catalog, String> {
    let c = parse_validated(text)?;
    if c.version <= baseline {
        return Err(format!("version {} 不比现有的 {baseline} 新", c.version));
    }
    Ok(c)
}

// ============================================================
// 内嵌 / 缓存 / 全局状态
// ============================================================

fn embedded() -> &'static Arc<Catalog> {
    static E: OnceLock<Arc<Catalog>> = OnceLock::new();
    E.get_or_init(|| {
        // 内嵌文件解析失败 = 构建就有问题，`embedded_json_is_valid` 会拦住它进发布。
        // 这里仍不 panic（release 是 panic=abort，目录又在热路径上）：退化成「什么都不认识」的空目录，
        // 效果是 accepts_image 一律 false —— 宁可不声明，也不崩。
        Arc::new(parse_validated(EMBEDDED).unwrap_or_else(|_| Catalog {
            catalog: CATALOG_NAME.into(),
            version: 0,
            updated: String::new(),
            default: String::new(),
            strong: String::new(),
            groups: Vec::new(),
            image_models: Vec::new(),
        }))
    })
}

fn cache_path() -> PathBuf {
    crate::installer::uking_home().join("cache").join("xiapan-models.json")
}

/// 读缓存：文件大小受限、合法、且 version 严格大于内嵌才认。任何异常都当「没有缓存」。
fn read_cache_at(path: &Path, embedded_version: u64) -> Option<Catalog> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() as usize > MAX_BYTES {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    adopt_candidate(&text, embedded_version).ok()
}

/// 临时文件 + rename：崩在写一半时，缓存要么是旧的完整版本、要么是新的完整版本，不会是半截。
fn write_cache_at(path: &Path, text: &str) -> Result<(), String> {
    let dir = path.parent().ok_or("缓存路径没有父目录")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("建缓存目录失败：{e}"))?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = path.with_file_name(format!(".xiapan-models.json.tmp.{}.{stamp}", std::process::id()));
    std::fs::write(&tmp, text).map_err(|e| format!("写临时文件失败：{e}"))?;
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("替换缓存失败：{e}"));
    }
    Ok(())
}

static EFFECTIVE: Mutex<Option<Effective>> = Mutex::new(None);

fn lock_state() -> std::sync::MutexGuard<'static, Option<Effective>> {
    // 毒化只说明别的线程在持锁时 panic 过；里面存的是不可变快照，照常接手。
    EFFECTIVE.lock().unwrap_or_else(|e| e.into_inner())
}

fn load_initial() -> Effective {
    let emb = embedded();
    // 测试里不读真实 ~/.uking/cache：开发机上有没有缓存、缓存是哪个版本，
    // 不许参与决定 providers 等模块的断言结果（缓存的读写另有显式路径的单测覆盖）。
    if !cfg!(test) {
        if let Some(c) = read_cache_at(&cache_path(), emb.version) {
            return Effective { source: Source::Cache, catalog: Arc::new(c) };
        }
    }
    Effective { source: Source::Embedded, catalog: Arc::clone(emb) }
}

// ============================================================
// 公开接口
// ============================================================

/// 当前生效的目录（第一次调用时按「内嵌 → 缓存」定下来；之后可能被后台刷新换成线上更新版）。
pub fn catalog() -> Effective {
    let mut g = lock_state();
    g.get_or_insert_with(load_initial).clone()
}

/// 这个模型 id 在虾盘云路由上**明确**能收图吗？目录里没有的、写了纯文本的，一律 false。
/// 只做精确匹配（id 区分大小写，`anthropic/…` 这类中转前缀名也不认）：
/// 不是同一个 id 就不是我们实测过的那个模型，不替它声明能力。
pub fn accepts_image(model_id: &str) -> bool {
    let id = model_id.trim();
    if id.is_empty() {
        return false;
    }
    catalog()
        .catalog
        .groups
        .iter()
        .flat_map(|g| g.items.iter())
        .any(|it| it.id == id && it.input.iter().any(|m| m == "image"))
}

/// 开箱即用的默认模型。
pub fn default_model() -> String {
    catalog().catalog.default.clone()
}

/// 「满血」档模型。
// 第一步只有测试在用它；等第二步把 Codex 档 / 强档路径里写死的 id 迁过来就有调用方了。
#[allow(dead_code)]
pub fn strong_model() -> String {
    catalog().catalog.strong.clone()
}

/// 只读动作 `runtime.model_catalog.inspect` 的返回体：生效的目录 + 来源 + 版本。
pub fn inspect() -> Result<Value, String> {
    let e = catalog();
    let c = &*e.catalog;
    let groups = serde_json::to_value(&c.groups).map_err(|err| format!("序列化目录失败：{err}"))?;
    let image_models = serde_json::to_value(&c.image_models).map_err(|err| format!("序列化目录失败：{err}"))?;
    Ok(json!({
        "catalog": c.catalog,
        "version": c.version,
        "updated": c.updated,
        "source": e.source.as_str(),
        "default": c.default,
        "strong": c.strong,
        "groups": groups,
        "image_models": image_models,
    }))
}

// ============================================================
// 后台刷新
// ============================================================

/// 按镜像顺序找第一个「合法且比 `baseline` 新」的线上副本；找到就写缓存并返回。
/// `fetch` 注入进来是为了单测不联网：生产传 [`fetch_text`]。
fn refresh_with(
    fetch: &dyn Fn(&str) -> Option<String>,
    urls: &[&str],
    cache: &Path,
    baseline: u64,
    budget: Duration,
) -> Option<Catalog> {
    let started = Instant::now();
    for url in urls {
        if started.elapsed() >= budget {
            break;
        }
        let Some(text) = fetch(url) else { continue };
        if text.len() > MAX_BYTES {
            continue;
        }
        let Ok(c) = adopt_candidate(&text, baseline) else { continue };
        // 缓存写失败不影响本次采用（内存里照样换新）；下次启动没缓存就再拉一次。
        let _ = write_cache_at(cache, &text);
        return Some(c);
    }
    None
}

fn fetch_text(url: &str) -> Option<String> {
    crate::installer::curl(&["-sL", "-m", "6", "--max-filesize", &MAX_BYTES.to_string(), url]).ok()
}

/// 拉一轮并（若有更新版）换进内存。阻塞，只在后台线程里调。
fn refresh_once() {
    let baseline = catalog().catalog.version;
    if let Some(c) = refresh_with(&fetch_text, CATALOG_URLS, &cache_path(), baseline, REFRESH_BUDGET) {
        let mut g = lock_state();
        // 拉取期间别的路径（比如缓存）可能已经换到更新的了：只前进、不倒退。
        if g.as_ref().map_or(true, |cur| c.version > cur.catalog.version) {
            *g = Some(Effective { source: Source::Online, catalog: Arc::new(c) });
        }
    }
}

/// 第一次调用返回 true、之后全是 false —— 「只起一次」的判据单独拎出来，好让单测直接打它。
fn claim_once(flag: &AtomicBool) -> bool {
    !flag.swap(true, Ordering::SeqCst)
}

/// 启动后台刷新。**幂等：进程里只会起一个线程**（重复调用直接返回）；线程内部有 [`REFRESH_BUDGET`]
/// 的总预算，每条镜像还有 curl 自己的 6s 超时；失败全部静默，不拦首屏、不弹任何东西。
pub fn start_background_refresh() {
    static STARTED: AtomicBool = AtomicBool::new(false);
    if !claim_once(&STARTED) {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("model-catalog-refresh".into())
        .spawn(|| {
            std::thread::sleep(START_DELAY);
            refresh_once();
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn embedded_value() -> Value {
        serde_json::from_str(EMBEDDED).expect("内嵌 JSON 必须是合法 JSON")
    }

    fn with_version(v: u64) -> String {
        let mut j = embedded_value();
        j["version"] = json!(v);
        j.to_string()
    }

    #[test]
    fn embedded_json_is_valid() {
        let c = parse_validated(EMBEDDED).expect("内嵌 xiapan-models.json 必须通过校验");
        assert_eq!(c.catalog, CATALOG_NAME);
        assert!(c.version >= 1);
        // 内嵌那份走的就是全局入口，不是只在单测里合法
        assert!(!embedded().groups.is_empty(), "embedded() 退化成空目录了 —— 内嵌 JSON 其实没解析成功");
    }

    /// 2026-10-03 逐个实测的结论，钉死在内嵌目录里。目录只收 7 个对话模型（glm-5.3-flash 10-04 下架）：
    /// 收图的声明 image，deepseek-v4-pro 实测拒绝看图只声明 text；目录外的一律不声明。
    #[test]
    fn embedded_encodes_the_2026_10_03_measurements() {
        let ids: Vec<&str> = embedded().groups.iter().flat_map(|g| g.items.iter()).map(|i| i.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "deepseek-v4-flash", "deepseek-v4-pro", "kimi-k3",
                "claude-sonnet-5-5", "claude-opus-5-5", "gpt-6-astra", "gpt-6.1-sol",
            ],
            "目录就是这 7 个对话模型，顺序即下拉顺序"
        );
        for id in [
            "deepseek-v4-flash", "kimi-k3",
            "claude-sonnet-5-5", "claude-opus-5-5", "gpt-6-astra", "gpt-6.1-sol",
        ] {
            assert!(accepts_image(id), "{id} 实测能看图，目录里必须声明 image");
        }
        assert!(!accepts_image("deepseek-v4-pro"), "deepseek-v4-pro 实测回答「无法查看图片」，不许声明 image");
        // 目录外的模型：哪怕实测过能看图，也不替它声明（只回答目录里的）
        for id in [
            "deepseek-flash", "deepseek-chat", "qwen3.7-flash", "qwen3.7-plus", "claude-opus-4-8", "gpt-5.4",
            "gpt-5.5", "gemini-3.1-pro-preview", "MiniMax-M3", "claude-sonnet-5", "claude-opus-5", "gpt-5.6-sol",
            // 实测不能看图的 / 已下线的
            "qwen3.7-max", "glm-5.3", "gemini-3.5-flash",
        ] {
            assert!(!ids.contains(&id), "{id} 不在这 8 个里");
            assert!(!accepts_image(id), "{id} 不在目录里，不许声明 image");
        }
        assert_eq!(embedded().default, "deepseek-v4-flash", "本次不改默认模型");
        assert_eq!(embedded().strong, "deepseek-v4-pro");
        // 贵档（U-Chat 折进「更多」）就是海外那组
        let pricey: Vec<&str> = embedded().groups.iter().filter(|g| g.pricey).flat_map(|g| g.items.iter()).map(|i| i.id.as_str()).collect();
        assert_eq!(pricey, ["claude-sonnet-5-5", "claude-opus-5-5", "gpt-6-astra", "gpt-6.1-sol"]);
    }

    /// 作图清单独立于对话清单：只有两个，不参与 `accepts_image`。
    #[test]
    fn embedded_image_models_are_separate_and_conservative() {
        let ids: Vec<&str> = embedded().image_models.iter().map(|m| m.id.as_str()).collect();
        // 2.5 选 flare 不选 1k：2026-10-04 实测 2.5-1k 只走百度渠道、9/27 后再没成功过，flare 另有可用渠道。
        assert_eq!(ids, ["gpt-image-2", "gpt-image-2.5-flare"]);
        for id in &ids {
            assert!(!accepts_image(id), "{id} 是作图模型，不属于「对话模型收不收图」");
        }
        let by_id = |id: &str| embedded().image_models.iter().find(|m| m.id == id).cloned().unwrap();
        assert!(by_id("gpt-image-2").edits, "gpt-image-2 是已验证的改图默认模型");
        assert!(!by_id("gpt-image-2.5-flare").edits, "2.5 只实测了文生图，改图端点没测过，不许声明支持改图");
        assert_eq!(by_id("gpt-image-2.5-flare").verified, "2026-10-04", "verified 只写真实测过的日期");
    }

    #[test]
    fn accepts_image_is_exact_and_conservative() {
        assert!(!accepts_image(""));
        assert!(!accepts_image("   "));
        assert!(!accepts_image("not-a-model"));
        // 大小写不同 / 中转前缀 = 不是我们实测过的那个 id，不替它声明
        assert!(!accepts_image("KIMI-K3"));
        assert!(!accepts_image("anthropic/claude-sonnet-5-5"));
        // 首尾空白无害
        assert!(accepts_image("  kimi-k3 "));
    }

    #[test]
    fn default_and_strong_come_from_the_catalog() {
        assert_eq!(default_model(), "deepseek-v4-flash");
        assert_eq!(strong_model(), "deepseek-v4-pro");
        assert!(accepts_image(&default_model()));
    }

    #[test]
    fn validate_rejects_bad_catalogs() {
        let base = embedded_value();
        let cases: Vec<(&str, Box<dyn Fn(&mut Value)>)> = vec![
            ("名字不对", Box::new(|j| j["catalog"] = json!("something-else"))),
            ("version 为 0", Box::new(|j| j["version"] = json!(0))),
            ("default 不存在", Box::new(|j| j["default"] = json!("no-such-model"))),
            ("strong 不存在", Box::new(|j| j["strong"] = json!("no-such-model"))),
            ("groups 为空", Box::new(|j| j["groups"] = json!([]))),
            ("input 含 video", Box::new(|j| j["groups"][0]["items"][0]["input"] = json!(["text", "video"]))),
            ("input 为空", Box::new(|j| j["groups"][0]["items"][0]["input"] = json!([]))),
            ("缺 input", Box::new(|j| { j["groups"][0]["items"][0].as_object_mut().unwrap().remove("input"); })),
            ("id 重复", Box::new(|j| {
                let dup = j["groups"][0]["items"][0]["id"].clone();
                j["groups"][0]["items"][1]["id"] = dup;
            })),
            ("id 带空白", Box::new(|j| j["groups"][0]["items"][0]["id"] = json!("a b"))),
            ("id 为空", Box::new(|j| j["groups"][0]["items"][0]["id"] = json!(""))),
            ("缺 label", Box::new(|j| j["groups"][0]["items"][0]["label"] = json!(" "))),
            ("分组没条目", Box::new(|j| j["groups"][0]["items"] = json!([]))),
            ("作图模型缺 edits", Box::new(|j| { j["image_models"][0].as_object_mut().unwrap().remove("edits"); })),
            ("作图模型 id 重复", Box::new(|j| {
                let dup = j["image_models"][0]["id"].clone();
                j["image_models"][1]["id"] = dup;
            })),
            ("作图模型 id 为空", Box::new(|j| j["image_models"][0]["id"] = json!(""))),
        ];
        for (name, mutate) in cases {
            let mut j = base.clone();
            mutate(&mut j);
            assert!(parse_validated(&j.to_string()).is_err(), "「{name}」应被拒绝");
        }
        assert!(parse_validated("not json").is_err());
        assert!(parse_validated("{}").is_err());
        assert!(parse_validated("[]").is_err());
    }

    #[test]
    fn catalog_without_image_models_is_still_valid() {
        // 老版本发出去的线上文件没有 image_models 字段，不能因此被整份丢弃
        let mut j = embedded_value();
        j.as_object_mut().unwrap().remove("image_models");
        let c = parse_validated(&j.to_string()).expect("缺 image_models 应按空处理");
        assert!(c.image_models.is_empty());
    }

    #[test]
    fn unknown_fields_are_ignored_for_forward_compat() {
        let mut j = embedded_value();
        j["future_field"] = json!({"x": 1});
        j["groups"][0]["items"][0]["future_item_field"] = json!(true);
        assert!(parse_validated(&j.to_string()).is_ok(), "新字段不该让老客户端整份丢弃");
    }

    #[test]
    fn adopt_requires_strictly_newer_version() {
        let v = embedded().version;
        assert!(adopt_candidate(&with_version(v), v).is_err(), "同版本不采用");
        assert!(adopt_candidate(&with_version(v), v + 1).is_err(), "更旧不采用");
        assert_eq!(adopt_candidate(&with_version(v + 1), v).unwrap().version, v + 1);
    }

    #[test]
    fn cache_roundtrip_and_every_failure_falls_back_to_none() {
        crate::testsandbox::with_sandbox("model-catalog-cache", &[], |root| {
            let path = root.join("cache").join("xiapan-models.json");
            let v = embedded().version;

            assert!(read_cache_at(&path, v).is_none(), "没文件 = 没缓存");

            write_cache_at(&path, &with_version(v + 3)).unwrap();
            assert_eq!(read_cache_at(&path, v).map(|c| c.version), Some(v + 3));
            assert!(read_cache_at(&path, v + 3).is_none(), "缓存不比内嵌新就不认");
            assert!(read_cache_at(&path, v + 9).is_none());

            // 覆盖写：rename 之后读到的是新内容，目录里不残留临时文件
            write_cache_at(&path, &with_version(v + 4)).unwrap();
            assert_eq!(read_cache_at(&path, v).map(|c| c.version), Some(v + 4));
            let leftovers: Vec<_> = std::fs::read_dir(path.parent().unwrap())
                .unwrap()
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.contains(".tmp."))
                .collect();
            assert!(leftovers.is_empty(), "临时文件没清掉：{leftovers:?}");

            // 坏 JSON / 过大 / 非法内容：全部当没缓存，不 panic
            std::fs::write(&path, "{ broken").unwrap();
            assert!(read_cache_at(&path, v).is_none());
            std::fs::write(&path, " ".repeat(MAX_BYTES + 1)).unwrap();
            assert!(read_cache_at(&path, v).is_none());
            let mut bad = embedded_value();
            bad["version"] = json!(v + 5);
            bad["default"] = json!("no-such-model");
            std::fs::write(&path, bad.to_string()).unwrap();
            assert!(read_cache_at(&path, v).is_none(), "default 悬空的缓存不能用");
        });
    }

    #[test]
    fn refresh_picks_first_valid_newer_mirror_and_writes_cache() {
        crate::testsandbox::with_sandbox("model-catalog-refresh", &[], |root| {
            let cache = root.join("cache").join("xiapan-models.json");
            let v = embedded().version;
            let seen = RefCell::new(Vec::<String>::new());
            let responses: Vec<(&str, Option<String>)> = vec![
                ("http://m1", None),                                   // 连不上
                ("http://m2", Some("<html>502</html>".into())),        // 不是 JSON
                ("http://m3", Some(with_version(v))),                  // 合法但不更新
                ("http://m4", Some(with_version(v + 2))),              // ← 该选它
                ("http://m5", Some(with_version(v + 9))),              // 不该走到
            ];
            let fetch = |url: &str| {
                seen.borrow_mut().push(url.to_string());
                responses.iter().find(|(u, _)| *u == url).and_then(|(_, r)| r.clone())
            };
            let urls: Vec<&str> = responses.iter().map(|(u, _)| *u).collect();
            let got = refresh_with(&fetch, &urls, &cache, v, Duration::from_secs(30)).expect("m4 合法且更新");
            assert_eq!(got.version, v + 2);
            assert_eq!(seen.borrow().len(), 4, "选中后不该再去拉后面的镜像：{:?}", seen.borrow());
            assert_eq!(read_cache_at(&cache, v).map(|c| c.version), Some(v + 2), "采用后必须写缓存");
        });
    }

    #[test]
    fn refresh_failures_leave_cache_untouched_and_return_none() {
        crate::testsandbox::with_sandbox("model-catalog-refresh-fail", &[], |root| {
            let cache = root.join("cache").join("xiapan-models.json");
            let v = embedded().version;
            let mut oversized = with_version(v + 1);
            oversized.push_str(&" ".repeat(MAX_BYTES + 1));
            let bad_default = {
                let mut j = embedded_value();
                j["version"] = json!(v + 1);
                j["default"] = json!("nope");
                j.to_string()
            };
            for (name, body) in [
                ("全部连不上", None),
                ("全是垃圾", Some("garbage".to_string())),
                ("过大", Some(oversized)),
                ("default 悬空", Some(bad_default)),
                ("版本不新", Some(with_version(v))),
            ] {
                let fetch = |_: &str| body.clone();
                assert!(
                    refresh_with(&fetch, &["http://a", "http://b"], &cache, v, Duration::from_secs(30)).is_none(),
                    "{name}：不该采用任何东西"
                );
                assert!(!cache.exists(), "{name}：失败时不许写缓存");
            }
        });
    }

    #[test]
    fn refresh_budget_stops_before_trying_more_mirrors() {
        let called = RefCell::new(0usize);
        let fetch = |_: &str| {
            *called.borrow_mut() += 1;
            None
        };
        let dir = std::env::temp_dir().join("uking-model-catalog-unused");
        let out = refresh_with(&fetch, &["http://a", "http://b", "http://c"], &dir, 1, Duration::ZERO);
        assert!(out.is_none());
        assert_eq!(*called.borrow(), 0, "预算为 0 时一条镜像都不该去碰");
    }

    #[test]
    fn mirror_urls_are_https_and_end_with_the_catalog_file() {
        assert!(!CATALOG_URLS.is_empty());
        for u in CATALOG_URLS {
            assert!(u.starts_with("https://"), "{u}");
            assert!(u.ends_with("/xiapan-models.json"), "{u}");
        }
    }

    #[test]
    fn inspect_reports_source_version_and_visible_groups() {
        let v = inspect().unwrap();
        assert_eq!(v["source"], "embedded");
        assert_eq!(v["catalog"], CATALOG_NAME);
        assert_eq!(v["version"], embedded().version);
        assert_eq!(v["default"], "deepseek-v4-flash");
        assert!(v["groups"].as_array().is_some_and(|g| !g.is_empty()));
        // 输出能被同一个解析器读回去（hook / 影子端拿到的就是这个形状）
        let groups: Vec<Group> = serde_json::from_value(v["groups"].clone()).unwrap();
        assert_eq!(groups, embedded().groups);
    }

    #[test]
    fn background_refresh_can_only_be_claimed_once() {
        // 不在测试里真起联网线程；钉住「只起一次」的判据本身。
        let flag = AtomicBool::new(false);
        assert!(claim_once(&flag), "第一次必须能抢到");
        assert!(!claim_once(&flag), "第二次不许再起");
        assert!(!claim_once(&flag));
    }
}
