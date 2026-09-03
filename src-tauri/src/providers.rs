//! 驱动切换（cc-switch 式）—— 改 Claude Code / Codex 的底层 API 指向。
//!
//! ## 思路（与 cc-switch 同构）
//!
//! - **Claude Code**：写 `~/.claude/settings.json` 的 `env` 块
//!   （ANTHROPIC_BASE_URL / ANTHROPIC_AUTH_TOKEN / ANTHROPIC_MODEL / ANTHROPIC_SMALL_FAST_MODEL）。
//!   只动我们管理的这几个键，其余配置原样保留；首次改动前备份 `settings.json.uking-bak`。
//! - **Codex**：写 `~/.codex/config.toml`（model_provider 指向自定义 provider，
//!   env_key="OPENAI_API_KEY"）+ `~/.codex/auth.json`（OPENAI_API_KEY=key）。
//!   覆盖前若无 U-King 标记则备份 `config.toml.uking-bak`。
//!   wire_api 按预设走：新版 Codex（CLI 与桌面 App）只认 "responses" —— 虾盘云有
//!   /v1/responses，但**能不能透传取决于模型挂在哪条渠道上**：`deepseek-v4-flash-codex`
//!   （type=1 直连渠道）200，裸名 `deepseek-v4-flash` 500 `convert_request_failed`。
//!   所以 Codex 链路默认模型是前者，不是贵几十倍的 gpt-5.3-codex；
//!   DeepSeek/GLM/Kimi 官方无 /v1/responses，仍写 "chat"（只兼容 0.8x 老版 CLI）。
//!   config.toml 是 Codex CLI 和桌面 App 共用的 —— 切一次驱动两边同时生效。
//!   responses 链路额外写 `requires_openai_auth=false` + `http_headers[x-openai-actor-authorization]`
//!   —— 修新版 Codex 桌面 App 接中转 provider 时 imagegen「生成完图不显示」（App 侧权限校验，见 apply_codex）。
//! - **预设**：虾盘云（内置充值，推荐）/ DeepSeek / 智谱 GLM / Kimi / Anthropic 官方（还原）。
//! - **实测连通**：用系统 curl 真实调一次模型（Anthropic 或 OpenAI 格式），返回模型回话 + 延迟。
//! - **沙箱**：环境变量 `UKING_TEST_HOME` 存在时，~/.claude 与 ~/.codex 重定向到该目录，
//!   方便在开发机上校验而不碰真实配置。

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use serde_yaml::{Mapping as YamlMapping, Value as YamlValue};
use std::path::PathBuf;
use std::time::Instant;

use crate::installer::curl;

// ============================================================
// 预设
// ============================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderPreset {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub summary: String,
    /// OpenAI 兼容端点（Codex 用）
    #[serde(default)]
    pub openai_base: String,
    /// Anthropic 原生端点（Claude Code 用）；None = 不支持 Claude Code
    #[serde(default)]
    pub anthropic_base: Option<String>,
    /// 默认模型（chat / ANTHROPIC_MODEL 同用）
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub small_model: String,
    /// Codex 链路专用模型（空 = 沿用 model）。新版 Codex 走 /v1/responses，
    /// 上游必须原生支持 Responses 格式，所以跟 Claude Code 链路的模型可以不同。
    #[serde(default)]
    pub codex_model: String,
    /// Codex 的 wire_api。🔴 **现在只有 "responses" 一个合法值**。
    /// 新版 Codex 已彻底移除 `wire_api = "chat"`：见到它不是「那个 provider 不可用」，
    /// 而是 **整份 config.toml 拒绝加载 → Codex 完全起不动**
    /// （`Error loading config.toml: wire_api = "chat" is no longer supported.`，Issue #364）。
    /// 字段保留只为兼容老配置文件的反序列化；写盘一律归一成 "responses"（见 `write_codex_config`）。
    #[serde(default = "default_wire_api")]
    pub codex_wire_api: String,
    /// 获取 Key / 充值 的网址
    #[serde(default)]
    pub key_url: String,
    #[serde(default)]
    pub key_hint: String,
    /// 是否走 U-King 内置充值（虾盘云）
    #[serde(default)]
    pub builtin_recharge: bool,
    /// 推荐排序权重
    #[serde(default)]
    pub recommended: bool,
    /// 内置预置（定义由我们维护、不可编辑；但**可以被用户从列表里移除**）。自定义 provider = false。
    /// `skip_deserializing` → 用户 json 里就算写了也忽略，由 read_custom 强制 false。
    #[serde(default, skip_deserializing)]
    pub builtin: bool,
    /// 自定义 provider 自带的 API Key（虾盘云/官方=空，走内置 Key 或不需要）。
    /// 切到自定义 provider 时前端把它一起传给 apply_provider。
    #[serde(default)]
    pub api_key: String,
}

fn default_wire_api() -> String {
    // 曾经默认 "chat"（兼容 0.8x 老 CLI）。新版 Codex 见到它整份配置都加载不了，
    // 而 0.8x 早已绝迹 —— 留着那个默认值只会持续制造「Codex 秒退」的客户机。
    WIRE_API.into()
}

/// 唯一合法的 wire_api 值。集中成常量，免得下次又在某个分支里漏掉一处写回 "chat"。
const WIRE_API: &str = "responses";

/// 默认**不**摆进列表的内置预置（0.9.85「列表只留两项」）。
///
/// 定义仍然留着 —— `all_providers` / `preset()` / 切驱动 / 回显 / 托盘全都照常认这些 id，
/// 只是**默认列表里不占位**。理由很直白：DeepSeek / GLM / Kimi 这三家「添加供应商」的
/// 模板里本来就有（`src/lib/providerTemplates.ts`），同一件事摆两遍就是干扰；Ollama 有自己的
/// 「本地大模型」页。默认只剩虾盘云（我们的生意）+ 官方直连（用户的退路），列表一眼看完。
///
/// 想要它们的用户，在「添加供应商」弹窗顶部一键加回（`prefs.shown` 记名，见 [`restore_provider_for`]）。
/// **不是删除**：三种情况下它们照样出现在列表里 —— ① 用户显式加回过；② 当前正被某个 AI
/// 工具使用（存量客户升级上来，列表不能把他在用的东西变没）；③ 用户自己排过序也不影响。
const SECONDARY_BUILTINS: &[&str] = &["deepseek", "glm", "kimi", "ollama"];

/// **有独立 provider 列表偏好**的 AI 工具（对应前端 `TOOL_TABS` 那几个页签）。
///
/// 🔴 这里曾写着「跟 `apply_provider` 的 targets 是同一批 id」—— 2026-08-03 上架
/// pi/qwen/crush/opencode 之后那句就不成立了：**能配驱动的是 9 个
/// （现在见 `APPLY_ALL_TARGETS`），有自己一份列表偏好的是这 6 个**。
/// 两件事碰巧曾经相等，不等于是同一个事实；靠一句「同一批」把它们绑在一起，
/// 后果就是加了四个工具、没人知道该不该同步改这里。要扩这份清单是独立的产品决定
/// （得先有「pi 的供应商列表」这个页面），不是跟着 apply 走。
/// 2026-08-22：加入 `pi` —— 前端「AI 设置」页新开了它的 Tab（跟 dsh 同一批理由：
/// `apply_provider` 早就支持 pi 这个 target，界面上一直没入口，客户配不上）。
/// `pi` 已经从 [`EXTRA_APPLY_TOOLS`] 数组里挪走了（见那边的注释），但 `driver_status()`
/// 里单独补了两行，让它照样出现在 `extra_installed`/`active`（「一键配好全部」弹窗要用）——
/// 两件事不冲突：这里管「有没有独立列表页」，那两行管「弹窗要不要单独列它」。
/// 2026-08-24：加入 `opencode` —— 同 pi/dsh 的理由。后端 `apply_opencode` 从 2026-08-03
/// 就在，`APPLY_ALL_TARGETS` 里也一直有它，但「AI 设置」没给 Tab、`tools.rs` 里还
/// `hidden: true`，于是它是「藏在『一键配好全部』里的隐藏项」：**客户既看不见也配不准**
/// （用户 2026-08-24 原话：「ai设置里边，你还要增加一个 opencode，在我的 ai 里边也要一个」）。
/// 🔴 **必须进这份清单，光加前端 Tab 不够** —— `check_tool`（本文件 `restore_provider_for` /
/// `hide_provider_for` 那几处调用）对不在 `LIST_TOOLS` 的 target 直接返回「未知的 AI 工具」，
/// Tab 能切驱动但列表增删改会当场报错。
/// 2026-08-29：加入 `cline` —— 同 pi/opencode 的第三批。apply_cline 当天新写（写
/// `~/.cline/data/settings/providers.json` 的 `openai-compatible` 槽位），AI 设置页同步开 Tab。
pub const LIST_TOOLS: &[&str] =
    &["claude", "codex", "clawx", "hermes", "dsh", "pi", "opencode", "cline"];

/// 用户**看得到**的 provider 列表 = （默认内置 − 被移除的 + 被加回的 + 在用的）+ 用户自定义，
/// 按用户排的序。
///
/// 「列表主权归用户」（0.9.84）：内置驱动不再是删不掉的常驻户 —— 移除了就是移除了，
/// 不会下次启动又自己回来，删光了就是空列表。加回来只能靠用户自己点（见 [`restore_provider_for`]）。
/// 我们过去在这里赖着不走，客户想用自己的 Key 却总被虾盘云挤回去，这是「抢用户模型」的根。
///
/// ★ **列表是 per-tool 的**（`tool = Some("claude"|"codex"|"clawx"|"hermes")`）：在 Claude Code
/// 那页删掉一个供应商，Hermes 那页照样留着。客户的原话是「不要一删除就全删除」——
/// 四个 AI 本来就各配各的驱动、各用各的模型，共用一份列表等于把「我不想让 Claude 用它」
/// 说成了「我不想再用它」。`tool = None` = 不分工具的全局视角（托盘 / 装机向导 / 自检用，
/// 它们本来就是一次性对多个工具动手）。
///
/// 排在第一位的就是首选/默认，顺序由用户自己调（`order`）。**没有自动故障转移** ——
/// 每个 AI 工具只认一份配置文件，客户端做不了转移；那件事在虾盘云服务端的跨渠道重试里做。
pub fn list_providers_for(tool: Option<&str>) -> Vec<ProviderPreset> {
    let view = view_prefs(&read_prefs(), tool);
    // 「正在用」的强制可见：存量客户可能已经切到 GLM/Kimi，升级后列表里不能把它变没
    // —— 那会让人以为配置丢了，回头又去重配一遍（比多显示一行糟糕得多）。
    // per-tool 视角下只看**这个工具**在用什么：Hermes 在用 GLM 不该把 GLM 塞进 Codex 的列表。
    let active = load_active_drivers();
    let in_use: Vec<String> = match tool {
        Some(tg) => active.get(tg).and_then(|v| v.as_str()).map(|s| vec![s.to_string()]).unwrap_or_default(),
        None => active.values().filter_map(|v| v.as_str().map(String::from)).collect(),
    };
    let mut all: Vec<ProviderPreset> = all_providers()
        .into_iter()
        .filter(|p| {
            // 用户显式移除的优先级最高 —— 在用也不摆回列表（主权归用户）。
            // 自定义也走这条：per-tool 移除**只从这个工具的列表里拿走**，定义和 Key 都还在
            // （别的工具照常用），要连定义一起删是另一条路（`tool=None`，界面上的「彻底删除」）。
            if view.hidden.iter().any(|h| h == &p.id) {
                return false;
            }
            if !p.builtin {
                return true;
            }
            if !SECONDARY_BUILTINS.contains(&p.id.as_str()) {
                return true;
            }
            view.shown.iter().any(|s| s == &p.id) || in_use.iter().any(|u| u == &p.id)
        })
        .collect();
    // 用户排过序的按 order 走，没排过的（新增的自定义、新版本新加的内置）留在原相对位置的后面。
    let rank = |id: &str| view.order.iter().position(|x| x == id).unwrap_or(usize::MAX);
    all.sort_by_key(|p| rank(&p.id));
    all
}

/// 不分工具的列表（托盘 / 装机向导 / 自检）。等价于 `list_providers_for(None)`。
pub fn list_providers() -> Vec<ProviderPreset> {
    list_providers_for(None)
}

/// 全集 = 内置预置 + 用户自定义，**不看移除/排序偏好**。
///
/// 跟 [`list_providers`] 的分工：那个回答「界面上该显示什么」，这个回答「这个 id 是什么」。
/// 按 id 取定义（apply / 测试 / 余额）一律走这里 —— 用户把虾盘云从列表里移走，是不想被它
/// 挤占，不是要让 `driver.apply xiapan` 这种**调用方显式指定**的路径失效；否则想切回去还得
/// 先「添加」一次，反而多一道坎。
pub fn all_providers() -> Vec<ProviderPreset> {
    let mut all = builtin_providers();
    all.extend(read_custom_providers());
    all
}

/// 虾盘云给 Codex 用的默认模型（单一真相源）。
///
/// 之所以要这么一个函数：同一句话原本抄在三处（这里的预设、`codex_proxy::DIRECT_MODEL`、
/// `uuswitch` 导给 uu-switch 的 config.toml），改一处漏两处就会出现「U-King 里跑便宜的、
/// 从 uu-switch 切一下变回贵的」。宪法第 8 条：同一事实存在几份就会漂移几份。
pub fn xiapan_codex_model() -> String {
    builtin_providers()
        .into_iter()
        .find(|p| p.id == "xiapan")
        .map(|p| p.codex_model)
        .unwrap_or_else(|| "deepseek-v4-flash-codex".into())
}

/// 虾盘云默认**对话**模型（非 Codex 链路）。同 [`xiapan_codex_model`] 的理由：凡是需要
/// 「U-King 默认用哪个模型」的地方一律来这里问，别各自写死一份 —— 定时任务、委派子进程、
/// 前端下拉曾经各写各的，结果客户在 AI 设置里看到 flash、无人值守的定时任务却在烧 pro。
pub fn xiapan_model() -> String {
    builtin_providers()
        .into_iter()
        .find(|p| p.id == "xiapan")
        .map(|p| p.model)
        .unwrap_or_else(|| "deepseek-v4-flash".into())
}

/// Resolve a provider for the isolated OpenClaw2 adapter. This is deliberately
/// read-only: it neither calls `apply_provider` nor changes any shared tool
/// configuration. `device_key` is supplied by lib.rs, the composition root.
pub(crate) fn resolve_openai_route_for_openclaw2(
    provider_id: &str,
    model_override: Option<&str>,
    explicit_key: Option<&str>,
    device_key: Option<&str>,
) -> Result<crate::openclaw2::ModelRoute, String> {
    let provider = all_providers()
        .into_iter()
        .find(|p| p.id == provider_id)
        .ok_or("invalid_input: OpenClaw2 未知 provider_id")?;
    let base = provider.openai_base.trim().to_string();
    let model = model_override
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.trim().to_string())
        .unwrap_or_else(|| provider.model.trim().to_string());
    if provider.id == "official" || base.is_empty() || model.is_empty() {
        return Err("invalid_input: OpenClaw2 provider 必须有 OpenAI endpoint 和对话模型".into());
    }
    let (key, key_source) = if let Some(key) = explicit_key.filter(|key| !key.trim().is_empty()) {
        (key.trim().to_string(), "explicit")
    } else if provider.builtin_recharge && is_xiapan_endpoint(&base) {
        let key = device_key
            .filter(|key| !key.trim().is_empty())
            .ok_or("not_ready: OpenClaw2 虾盘云设备钱包不可用")?;
        (key.trim().to_string(), "device_wallet")
    } else if !provider.builtin && !provider.api_key.trim().is_empty() {
        (provider.api_key.trim().to_string(), "stored")
    } else if is_loopback_openai_base(&base) {
        ("openclaw2-loopback".into(), "loopback_placeholder")
    } else {
        return Err("invalid_input: OpenClaw2 远程 provider 需要显式或已保存 API Key".into());
    };
    Ok(crate::openclaw2::ModelRoute {
        source_id: provider.id,
        source_name: provider.name,
        base,
        model,
        key,
        key_source: key_source.into(),
    })
}

fn is_loopback_openai_base(base: &str) -> bool {
    let lower = base.trim().to_ascii_lowercase();
    let Some((scheme, rest)) = lower.split_once("://") else { return false };
    if scheme != "http" && scheme != "https" { return false; }
    let host = rest.split(['/', '?', '#', '\\']).next().unwrap_or("")
        .rsplit_once('@').map(|(_, host)| host).unwrap_or(rest);
    let host = host.split(':').next().unwrap_or(host).trim_matches(['[', ']']);
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

/// 定时任务（无人值守）用哪个模型。**跟随客户在「AI 设置」里选的那个**，不写死。
///
/// 这里曾经直接用 [`xiapan_model`]（flash），理由是「无人值守最该省钱」。代价是：
/// 客户在 AI 设置里看到的是 A，半夜替他干活的是 B，产出质量对不上他的预期，而他
/// **无从知道为什么**。省钱省在最没人看着的地方，等于把「不对劲」藏起来 ——
/// 客户报的「定时任务跑出来的东西不对」，这是头号嫌疑。
///
/// 只在**客户的 Claude 当前就走虾盘云**时才跟随它的模型：定时任务这条路固定打
/// 虾盘云端点，他要是切了官方直连 / 自己的中转，那个模型名在我们端点上根本不存在，
/// 跟随过去只会 404。这种情况回落到虾盘云默认模型（宁可回落，也不要报一个假错）。
pub fn automation_model() -> String {
    let st = driver_status();
    // 按「谁当前正走虾盘云」挑一个可跟随的模型。定时任务这条路固定打虾盘云端点，
    // 所以**只有走虾盘云的那些工具**的模型名，在这个端点上才真实存在；客户把某个工具
    // 切回官方直连 / 自己的中转后，那边的模型名跟过来只会 404。
    //
    // 只看 claude 一个是不够的：实测开发机上 claude=official 而 clawx/hermes=xiapan，
    // 这种客户「跟随设置」会完全不生效、永远吃默认值 —— 那就等于没做。
    // **故意不含 codex**：它的模型名（gpt-5.x-codex 那一族）是给 responses 协议的编程模型，
    // 不适合拿来跑「写条文案 / 出张图」这种通用任务。
    for (tool, model) in [
        ("claude", st.claude_model.as_deref()),
        ("clawx", st.clawx_model.as_deref()),
        ("hermes", st.hermes_model.as_deref()),
    ] {
        if st.active.get(tool).map(String::as_str) == Some("xiapan") {
            if let Some(m) = model.map(str::trim).filter(|m| !m.is_empty()) {
                // ClawX / Hermes 存的是**带命名空间的 id** —— 实测是
                // `custom-ukingxia/deepseek-v4-pro`，那是它们自己配置文件里的写法。
                // 虾盘云 API 要的是裸模型名，原样送过去会得到
                // `No available channel for model custom-ukingxia/...`（2026-08-04 真跑撞到）。
                // 取最后一段；本来就是裸名的（Claude 那边）经过这一步不变。
                let bare = m.rsplit('/').next().unwrap_or(m).trim();
                if !bare.is_empty() {
                    return bare.to_string();
                }
            }
        }
    }
    xiapan_model()
}

/// 给对话页「委派 `claude -p` / `codex exec`」用：把虾盘云端点 + 对话当前 Key 拼成子进程 env，
/// 让委派出去的编程 agent **免配置**直接用同一套计费（客户没单独配过 Claude/Codex 也能跑通）。
/// 端点和模型**全部**读虾盘云 preset —— 这里曾经把 ANTHROPIC_MODEL 单独写死成 `deepseek-v4-pro`
/// （理由是「满血 pro 比 flash 会写代码」），结果是同一台机器上「切驱动写进 settings.json 的」
/// 和「委派子进程 env 里的」是两个不同模型，客户在 AI 设置里看到 flash、账单上却是 pro。
/// 宪法第 8 条：同一事实存在几份就会漂移几份。要换默认模型只改 preset 一处。
/// `api_key` = 对话页在用的 Key（设备指纹 Key 或 env Key）。
///
/// 🔴 **客户已经在用自己的 Key / 官方登录时，对应那一半一个字都不注入**（issue #375）。
///
/// 这里原来是无条件全量注入的，后果是：客户在「AI 设置」里把 Claude Code 切到自己的
/// 中转、界面也如实显示着他自己那家，可只要他从工作台委派一次 `claude -p`，
/// 子进程 env 里被我们盖上虾盘云端点 + **设备 Key**，这一轮就记在我们账上。
/// 客户看到的是「我明明选了自己的 key，还在扣虾盘云的 token」—— 而且他没做错任何事。
/// 这撞的是产品红线「不许抢客户的模型 / 登录态」，不是「体验不够好」。
///
/// 判据直接复用 [`claude_owns_config`] / [`codex_owns_config`] —— 前端「不推虾盘云、
/// 不弹接管提示」用的就是这两个，同一个事实不许有第二套判据（宪法第 8 条）。
/// 它们的保守偏向也正合适：**拿不准一律当成「是用户自己的」**，宁可少注入让他自己配，
/// 也绝不替他花钱。
///
/// 两半**分开判**：只切了 Claude Code 的客户，Codex 那半边照旧免配置直连。
pub fn delegation_env(api_key: &str) -> Vec<(String, String)> {
    let xp = builtin_providers().into_iter().find(|p| p.id == "xiapan");
    let anthropic = xp.as_ref().and_then(|p| p.anthropic_base.clone()).unwrap_or_else(|| "https://api.u-claw.org.cn".into());
    let openai = xp.as_ref().map(|p| p.openai_base.clone()).unwrap_or_else(|| "https://api.u-claw.org.cn/v1".into());
    let model = xp.as_ref().map(|p| p.model.clone()).unwrap_or_else(|| "deepseek-v4-flash".into());
    let small = xp.as_ref().map(|p| p.small_model.clone()).unwrap_or_else(|| "deepseek-v4-flash".into());
    let mut v: Vec<(String, String)> = Vec::new();
    if !claude_owns_config() {
        v.push(("ANTHROPIC_BASE_URL".into(), anthropic));
        v.push(("ANTHROPIC_AUTH_TOKEN".into(), api_key.to_string()));
        v.push(("ANTHROPIC_MODEL".into(), model));
        v.push(("ANTHROPIC_SMALL_FAST_MODEL".into(), small));
        v.push(("API_TIMEOUT_MS".into(), "600000".into()));
        // 这一支按定义就是虾盘云 DeepSeek 路由，与 apply_claude_to 的 200K 默认对齐
        // （宪法第 8 条：同一事实不许两套口径）。用户自己的配置分支不受影响。
        v.push(("CLAUDE_CODE_AUTO_COMPACT_WINDOW".into(), DEEPSEEK_AUTO_COMPACT_WINDOW.into()));
        v.push(("CLAUDE_CODE_MAX_CONTEXT_TOKENS".into(), DEEPSEEK_AUTO_COMPACT_WINDOW.into()));
    }
    if !codex_owns_config() {
        v.push(("OPENAI_API_KEY".into(), api_key.to_string()));
        v.push(("OPENAI_BASE_URL".into(), openai));
    }
    v
}

/// 这一轮（对某个大脑）是不是虾盘云委托：`delegation_env` 对**这个工具**真注了 env 才算。
///
/// 🔴 判据必须按工具分开 —— `delegation_env` 返回的列表是 claude 半边 + codex 半边拼的，
/// 只要有一半没自持凭据它就非空；拿「列表非空」判 claude 轮，会在「codex 自持、claude 也自持」
/// 的机器上误判成委托（claude 明明直连官方却按镜像策略清代理，bug 复活）。
pub fn claude_delegated() -> bool {
    !claude_owns_config()
}

pub fn codex_delegated() -> bool {
    !codex_owns_config()
}

/// 内置预置（硬编码，**定义**不可改 —— 但用户可以把它从自己的列表里移除，见 [`remove_provider_for`]）。
/// 每个 builtin=true。硬编码是为了端点/模型日后还能随版本更新到存量客户。
fn builtin_providers() -> Vec<ProviderPreset> {
    let mut v = vec![
        ProviderPreset {
            id: "xiapan".into(),
            name: "虾盘云（U-King 内置）".into(),
            summary: "开箱即用，国内直连。默认 DeepSeek-V4 Flash（快·稳·省，聊天不卡不断），要满血推理在「换模型」里切 Pro；Codex 默认走 DeepSeek 省钱路由（GPT-5.3-Codex 等海外模型价格贵几十倍：可自选模型直连，或联系客服对接）。".into(),
            // ⚠️ 必须用 api.u-claw.org.cn（国内镜像）：裸 api.u-claw.org 国内 GFW SNI reset，
            // 客户机连不上（pc-*** 2026-06-17 实测 HTTP 000）。镜像反代新加坡，chat/作图/余额全功能。
            openai_base: "https://api.u-claw.org.cn/v1".into(),
            anthropic_base: Some("https://api.u-claw.org.cn".into()),
            // 默认走 flash（非推理型）：pro 是推理模型，输出预算一紧就把 token 烧在 reasoning 上、
            // 正文返回空 → openclaw 判 incomplete turn（stopReason=length）→ 客户看到「无法生成回复，
            // 请重试」（2026-07-10 Mac 客户机实测 + curl 复现：pro 档正文空，flash 15s 完整）。
            // flash 又快(约pro一半)又省钱(约1/3)，微信助手/办公足够；要满血 pro 在「换模型」下拉里切。
            model: "deepseek-v4-flash".into(),
            small_model: "deepseek-v4-flash".into(),
            // Codex 走 /v1/responses（CLI 和桌面 App 都只认这个协议）。
            //
            // 默认模型 = `deepseek-v4-flash-codex`，**不是** gpt-5.3-codex：后者贵得多
            //（2026-08-02 查线上 ModelRatio：gpt-5.3-codex 9.58125 vs flash 1.5，输入价约 6.4 倍；
            // 别再跟着老注释写「几十倍」，那个数没人核过）。服务端为此单独建了一条 type=1 渠道只挂这个名字
            // （老的 DeepSeek 类渠道 type=43 会试图把 responses 转成 chat，那个转换没实现，
            // 直接 500 `convert_request_failed` —— 2026-08-02 实测：裸名 `deepseek-v4-flash`
            // 打 /v1/responses 就是这个错，带 `-codex` 后缀才 200）。
            //
            // 这跟 `codex_proxy::DIRECT_MODEL` 是**同一个值**：那边是「切模型路由」时写的配置，
            // 这边是「切驱动」时写的，两条路都可能先落地，写不一样就会出现「界面写着 A、
            // 实际跑的是 B」。要海外 GPT 的用户在「换模型」下拉里显式选 gpt-5.3-codex，
            // `ensure_codex_cheap_route` 见到非 deepseek 模型会尊重直连、不接管。
            codex_model: "deepseek-v4-flash-codex".into(),
            codex_wire_api: "responses".into(),
            // 国内可达充值页（cloud.u-claw.org 国内 SNI 被阻断，见 device.rs 注释）
            key_url: "https://u-claw.org.cn/recharge".into(),
            key_hint: "sk- 开头，充值页可见".into(),
            builtin_recharge: true,
            recommended: true,
            builtin: true,
            api_key: String::new(),
        },
        // 注：原「虾盘云·Claude 版」「虾盘云·ChatGPT 版」已合并删除 —— 它们底层是同一个
        // 虾盘云、同一个 Key，只是默认模型不同，列 3 个对小白是干扰（实测用户反馈「重复了」）。
        // 现在只留 1 个虾盘云，Claude/GPT/DeepSeek 等模型全在「换模型」下拉里切（XIAPAN_MODELS）。
        ProviderPreset {
            id: "deepseek".into(),
            name: "DeepSeek 官方".into(),
            summary: "国内最快最便宜，需自行注册拿 Key。官方支持 Claude Code 和 Codex 接入。".into(),
            openai_base: "https://api.deepseek.com/v1".into(),
            anthropic_base: Some("https://api.deepseek.com/anthropic".into()),
            // deepseek-chat/deepseek-reasoner 官方 2026/07/24 弃用且无兜底 → 用现役 v4 名字
            model: "deepseek-v4-flash".into(),
            small_model: "deepseek-v4-flash".into(),
            // ★ Codex 可用（2026-08-11 在客户机 pc-*** 用他自己的 Key 实测）。
            //
            // 这里曾经写着「DeepSeek 官方无 /v1/responses，新版 Codex 接不了」——
            // 那句在 2026-07-31 之前是对的，之后就过期了：官方那天上线 V4-Flash 正式版，
            // **原生支持 OpenAI Responses API**，Codex CLI / 桌面 App / VS Code 扩展直连即可，
            // 不再需要本地翻译代理（`codex_proxy.rs` 那条路是那段空窗期的唯一解法）。
            //
            // 实测（客户机 curl，我们写出去的配置原样打）：
            //   /v1/responses + deepseek-v4-flash            → 200 ✅（带不带 /v1 都通）
            //   再加 store:false / 我们那个自定义头          → 200 ✅（两个额外键都不碍事）
            //   /v1/responses + deepseek-v4-pro              → 400 ❌ 官方原话：
            //     "Codex integration with deepseek-v4-pro will be available starting
            //      early August 2026. Please use deepseek-v4-flash instead for now."
            //
            // 🔴 所以 `codex_model` **显式钉死 flash，不能留空**。留空会回退到 `model` ——
            // 今天两者恰好相同，但哪天有人把 `model` 升成 v4-pro（很有诱惑力，它更强），
            // Codex 就会**静默 400**，而 Claude Code 那条链路照常能用，
            // 排查的人根本想不到是被隔壁一行改坏的。v4-pro 的 Codex 支持上线后再改这里。
            codex_model: "deepseek-v4-flash".into(),
            codex_wire_api: WIRE_API.into(),
            key_url: "https://platform.deepseek.com/api_keys".into(),
            key_hint: "sk- 开头".into(),
            builtin_recharge: false,
            recommended: false,
            builtin: true,
            api_key: String::new(),
        },
        ProviderPreset {
            id: "glm".into(),
            name: "智谱 GLM".into(),
            summary: "有免费额度，官方支持 Claude Code 接入。".into(),
            openai_base: "https://open.bigmodel.cn/api/paas/v4".into(),
            anthropic_base: Some("https://open.bigmodel.cn/api/anthropic".into()),
            model: "glm-5".into(),
            small_model: "glm-5-flash".into(),
            codex_model: "".into(),
            codex_wire_api: WIRE_API.into(),
            key_url: "https://open.bigmodel.cn/usercenter/apikeys".into(),
            key_hint: "在智谱开放平台「API 密钥」页创建".into(),
            builtin_recharge: false,
            recommended: false,
            builtin: true,
            api_key: String::new(),
        },
        ProviderPreset {
            id: "kimi".into(),
            name: "Kimi（月之暗面）".into(),
            summary: "超长上下文，官方支持 Claude Code 接入。".into(),
            openai_base: "https://api.moonshot.cn/v1".into(),
            anthropic_base: Some("https://api.moonshot.cn/anthropic".into()),
            model: "kimi-k2.6".into(),
            small_model: "kimi-k2.6".into(),
            codex_model: "".into(),
            codex_wire_api: WIRE_API.into(),
            key_url: "https://platform.moonshot.cn/console/api-keys".into(),
            key_hint: "sk- 开头，在「API Keys」页创建".into(),
            builtin_recharge: false,
            recommended: false,
            builtin: true,
            api_key: String::new(),
        },
        ProviderPreset {
            id: "ollama".into(),
            name: "本地大模型（Ollama）".into(),
            summary: "离线 · 免费 · 数据不出本机。在自己电脑跑开源大模型，断网也能用，适合隐私敏感场景。效果受机器配置限制，重活仍建议虾盘云。".into(),
            // Ollama 的 OpenAI 兼容端点（本机 11434）。⚠️ 只有 OpenAI 格式，没有 Anthropic
            // /v1/messages，所以 Claude Code 接不了；新版 Codex 只认 responses 也接不了 ——
            // 本地模型只配进 ClawX / Hermes（都是 OpenAI 兼容、可改 baseUrl），外加终端直接 ollama run。
            openai_base: "http://localhost:11434/v1".into(),
            anthropic_base: None,
            // model 随机器推荐而变（hardware.rs 给档位），切换时由 model_override 传入；这里给个稳妥默认
            model: "qwen2.5:7b".into(),
            small_model: "qwen2.5:3b".into(),
            codex_model: "".into(),
            codex_wire_api: "".into(),
            // 本地无需 Key（端点不校验），UI 不引导获取 Key
            key_url: "".into(),
            key_hint: "本地模型无需 API Key".into(),
            builtin_recharge: false,
            recommended: false,
            builtin: true,
            // 占位 Key：Ollama 的 OpenAI 端点不校验，但 ClawX/Hermes 配置项要求非空
            api_key: "ollama".into(),
        },
        // 🔴 这里曾有个 `zen-free`（Ox Alpha 免费尝鲜）内置预设，2026-08-24 当天加、当天退，
        // 没进过任何发布版。退的理由不是它不能用（pc-*** 实测通），是**它放错了层**：
        // 免费羊毛的寿命以周计（stealth 预览随时撤、免费额度随时收），而内置预设改一次要发一次版。
        // 免费这件事整体挪到「添加供应商」模板画廊 + AI 设置里的教程页，两者都走 skill 清单
        // 热下发（`installer.rs::RemoteProviderTemplate`）—— 换一家免费模型只改线上 JSON。
        // 用户 2026-08-24 原话：「不要每次都改模型供应商，以不变应万变」。
        ProviderPreset {
            id: "official".into(),
            name: "官方直连（还原）".into(),
            summary: "清除 U-King 写入的配置，还原 Anthropic / OpenAI 官方登录。".into(),
            openai_base: "".into(),
            anthropic_base: None,
            model: "".into(),
            small_model: "".into(),
            codex_model: "".into(),
            codex_wire_api: "".into(),
            key_url: "https://claude.ai".into(),
            key_hint: "".into(),
            builtin_recharge: false,
            recommended: false,
            builtin: true,
            api_key: String::new(),
        },
    ];
    // 统一标记内置（上面逐个写 builtin:true 已足够，这里兜底防漏）
    for p in &mut v {
        p.builtin = true;
    }
    v
}

fn preset(id: &str) -> Result<ProviderPreset, String> {
    all_providers()
        .into_iter()
        .find(|p| p.id == id)
        .ok_or_else(|| format!("未知驱动 {id}"))
}

// ============================================================
// 自定义 provider 持久化（~/.uking/providers.json）
// ============================================================
//
// 内置预置硬编码不可改；用户自定义的中转站存这个 json，list_providers 读时合并。
// 纯 serde_json，零新依赖。支持 UKING_TEST_HOME 沙箱（走 config_home）。

/// 自定义 provider 存储路径：`~/.uking/providers.json`。
fn custom_providers_path() -> PathBuf {
    config_home().join(".uking").join("providers.json")
}

/// 读用户自定义 provider（不存在/解析失败都返回空，绝不让主流程崩）。
/// 强制 builtin=false（结构体里 skip_deserializing 已保证，这里再兜底），
/// 并过滤掉与内置同 id 的项（防止用户文件覆盖虾盘云等预置）。
fn read_custom_providers() -> Vec<ProviderPreset> {
    let builtin_ids: Vec<String> = builtin_providers().into_iter().map(|p| p.id).collect();
    let Ok(s) = std::fs::read_to_string(custom_providers_path()) else {
        return Vec::new();
    };
    let mut list: Vec<ProviderPreset> = serde_json::from_str(&s).unwrap_or_default();
    list.retain(|p| !p.id.trim().is_empty() && !builtin_ids.contains(&p.id));
    for p in &mut list {
        p.builtin = false;
        // 🔴 **读取侧也要 trim —— 这是给存量机器的自愈路径。**
        // `save_custom_provider` 那一处只管「以后存进来的干净」，救不了**已经**带着
        // 前导空格躺在 providers.json 里的那些（本机实测就有一条：
        // `" https://opencode.ai/zen/go/v1"`）。老用户不会因为我们发了新版就重新
        // 粘贴一遍端点 —— 不在读取侧修，那条脏数据会一直配出打不通的驱动，
        // 而界面上「https://…」和「 https://…」长得一模一样，没人能看出来。
        //
        // 只 trim 不改文件：读到的干净就够用了，不为此产生一次用户没要求的写入
        // （宪法 10 —— 别静默改用户的文件；下次他自己保存时会顺带落盘）。
        for s in [
            &mut p.name,
            &mut p.openai_base,
            &mut p.model,
            &mut p.small_model,
            &mut p.codex_model,
            &mut p.api_key,
        ] {
            let t = s.trim().to_string();
            *s = t;
        }
        if let Some(a) = p.anthropic_base.as_mut() {
            let t = a.trim().to_string();
            *a = t;
        }
    }
    list
}

/// 写回自定义 provider 列表（原子写：temp + rename）。
fn write_custom_providers(list: &[ProviderPreset]) -> Result<(), String> {
    let path = custom_providers_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建 .uking 目录失败: {e}"))?;
    }
    let tmp = path.with_extension("json.tmp");
    let body = serde_json::to_string_pretty(list).map_err(|e| format!("序列化失败: {e}"))?;
    std::fs::write(&tmp, body).map_err(|e| format!("写 providers.json 失败: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("替换 providers.json 失败: {e}"))
}

/// slug 化：把名字转成合法的 provider id（只留字母数字和连字符，小写）。
fn slugify(s: &str) -> String {
    let out: String = s
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        "provider".into()
    } else {
        out
    }
}

/// 自定义 provider id 会进入 TOML 表名，不能允许引号、换行或 `.` 改写配置结构。
fn valid_provider_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 80
        && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// API Key、名称、URL 都是粘贴输入，写 TOML 前统一转义，避免破坏 config.toml。
fn toml_basic_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// 新增 / 更新一个自定义 provider（upsert，按 id）。
/// 拒绝操作内置 id（防止覆盖虾盘云/DeepSeek 等预置）。
pub fn save_custom_provider(mut p: ProviderPreset) -> Result<ProviderPreset, String> {
    // 🔴 **在存的时候 trim，不要在每个 `apply_*` 里各 trim 一遍。**
    // 这些字段全是用户从网页粘贴进来的，前后带空格是常态。2026-08-24 客户机上实测落盘的是
    // `" https://opencode.ai/zen/go/v1"`（前导一个空格），一路裸奔进了 pi 的 models.json。
    // 空格不会报错、界面上也看不出来（HTML 里 `" x"` 和 `"x"` 长得一样），只会让请求发不出去，
    // 而客户看到的是「配好了但用不了」。九个 apply_* 各写一遍 trim 是九份会漂的实现
    // （宪法 12：公共能力复用不复制）—— 真相源只有一个：**存进来的就该是干净的**。
    for s in [
        &mut p.name,
        &mut p.openai_base,
        &mut p.model,
        &mut p.small_model,
        &mut p.codex_model,
        &mut p.api_key,
    ] {
        let t = s.trim().to_string();
        *s = t;
    }
    if let Some(a) = p.anthropic_base.as_mut() {
        let t = a.trim().to_string();
        *a = t;
    }

    let builtin_ids: Vec<String> = builtin_providers().into_iter().map(|x| x.id).collect();

    // id 为空 → 用 name slug 化自动生成；带 custom- 前缀避免和未来内置撞名
    if p.id.trim().is_empty() {
        // 🔴 中文名 slug 化后是空的（slugify 兜底成 `provider`）→ **所有中文名供应商都会撞成
        // 同一个 id**，而本函数是 upsert 按 id：第二个中文名供应商会**静默覆盖**第一个，
        // 用户看到的是「我加的那个供应商没了」。撞了就加序号，保证唯一。
        // （issue #359 客户机上的 id 是 `custom--` —— 那是前端自己算 id 留下的老形状，
        //  前端那份已删，现在一律由后端生成，判据只此一处。）
        let base = format!("custom-{}", slugify(&p.name));
        let taken = read_custom_providers();
        let mut id = base.clone();
        let mut n = 2;
        while taken.iter().any(|x| x.id == id) || builtin_ids.contains(&id) {
            id = format!("{base}-{n}");
            n += 1;
        }
        p.id = id;
    }
    if builtin_ids.contains(&p.id) {
        return Err(format!("「{}」是内置驱动，不能修改或覆盖", p.name));
    }
    if !valid_provider_id(&p.id) {
        return Err("provider id 只能包含英文字母、数字、连字符或下划线（最长 80 位）".into());
    }
    if p.name.trim().is_empty() {
        return Err("provider 名称不能为空".into());
    }
    if p.openai_base.trim().is_empty() && p.anthropic_base.as_deref().unwrap_or("").trim().is_empty()
    {
        return Err("至少要填一个端点（OpenAI 兼容地址 或 Anthropic 地址）".into());
    }
    p.builtin = false;
    // 归一：新版 Codex 只认 responses，写别的会让整份 config.toml 加载失败（#364）
    if p.codex_wire_api.trim() != WIRE_API {
        p.codex_wire_api = WIRE_API.into();
    }

    let mut list = read_custom_providers();
    if let Some(slot) = list.iter_mut().find(|x| x.id == p.id) {
        *slot = p.clone();
    } else {
        list.push(p.clone());
    }
    write_custom_providers(&list)?;
    Ok(p)
}

/// 删除一个自定义 provider（内置不可删）。
pub fn delete_custom_provider(id: &str) -> Result<(), String> {
    let builtin_ids: Vec<String> = builtin_providers().into_iter().map(|x| x.id).collect();
    if builtin_ids.contains(&id.to_string()) {
        return Err("内置驱动不能删除".into());
    }
    let mut list = read_custom_providers();
    let before = list.len();
    list.retain(|p| p.id != id);
    if list.len() == before {
        return Err(format!("没找到自定义 provider「{id}」"));
    }
    write_custom_providers(&list)
}

// ============================================================
// 列表偏好：移除的内置（墓碑）+ 排序（~/.uking/provider-prefs.json）
// ============================================================
//
// ## 为什么内置用「墓碑」而不是把它们写进用户文件（0.9.84）
//
// 目标是「删了就是删了、永不自己回来」。两种实现都能做到，但把 6 个内置一次性种进
// providers.json 会**冻结定义**：虾盘云的端点、默认模型日后就改不动了。这不是假想 ——
// `api.u-claw.org` 被 GFW SNI 阻断那次（pc-***），全靠改硬编码端点换成 `u-claw.org.cn`
// 才把存量客户救回来；要是端点已经躺在每台机器的用户文件里，那种修复一台也到不了。
//
// 所以内置仍是硬编码（可随版本更新），另存一份**被移除的 id 名单**。对用户完全等价：
// 移除后列表里没有、重启也不会回来（墓碑是持久的，不是「本次会话隐藏」）；
// 加回来只能靠用户自己点「添加」。
//
// ⚠️ 铁律：**任何地方都不准自动往 hidden 外删 id**（即不准替用户把驱动加回来）。
// 想加回来的唯一入口是用户显式调 [`restore_provider_for`]。种子自动补全 = 又一次抢用户的列表。

/// 一个 AI 工具的列表偏好。`hidden` = 被移出这个工具列表的 id（内置立墓碑、自定义也只是
/// 移出，定义不动）；`shown` 是 [`SECONDARY_BUILTINS`] 的反向名单（默认不摆出来的内置，
/// 用户点过「添加」才记名）；`order` 是显示顺序。三份名单都**只由用户显式操作写**，
/// 没有任何自动补种路径。
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct ToolListPrefs {
    #[serde(default)]
    hidden: Vec<String>,
    #[serde(default)]
    shown: Vec<String>,
    #[serde(default)]
    order: Vec<String>,
}

/// 列表偏好总账。
///
/// `tools` 是**每个 AI 一份**（0.9.9x）；顶层那三个字段是 0.9.8x 时代所有工具共用的那一份，
/// 现在只有两个用途：① 某个工具还没有自己的偏好时按它算（存量客户升级上来，四个页面看到的
/// 列表跟升级前一字不变）；② `tool=None` 的全局视角（托盘 / 装机向导）。
/// 用户第一次在某个工具页里删 / 加 / 排序，就把这份全局的**复制**成那个工具的，之后各走各的
/// —— 不做原地改写，否则一次「在 Claude 里删掉」会顺手改掉另外三个工具的历史。
#[derive(Debug, Default, Serialize, Deserialize)]
struct ListPrefs {
    #[serde(default)]
    hidden: Vec<String>,
    #[serde(default)]
    shown: Vec<String>,
    #[serde(default)]
    order: Vec<String>,
    #[serde(default)]
    tools: std::collections::BTreeMap<String, ToolListPrefs>,
}

/// 0.9.8x 那份全局偏好，当成「还没分家的工具」的默认视图。
fn legacy_view(p: &ListPrefs) -> ToolListPrefs {
    ToolListPrefs { hidden: p.hidden.clone(), shown: p.shown.clone(), order: p.order.clone() }
}

/// 某个工具（或全局）当前该按哪份偏好显示。只读，不落盘、不迁移。
fn view_prefs(prefs: &ListPrefs, tool: Option<&str>) -> ToolListPrefs {
    tool.and_then(|t| prefs.tools.get(t).cloned())
        .unwrap_or_else(|| legacy_view(prefs))
}

/// 拿某个工具**可写**的偏好槽位；它还没分家就先从全局那份复制一份（迁移只发生在第一次写）。
fn tool_slot<'a>(prefs: &'a mut ListPrefs, tool: &str) -> &'a mut ToolListPrefs {
    if !prefs.tools.contains_key(tool) {
        let seed = legacy_view(prefs);
        prefs.tools.insert(tool.to_string(), seed);
    }
    prefs.tools.get_mut(tool).expect("刚插进去的槽位")
}

/// 工具 id 合法性 —— 打错一个字就该当场报错，不能静默去改另一份偏好。
fn check_tool(tool: &str) -> Result<(), String> {
    if LIST_TOOLS.contains(&tool) {
        Ok(())
    } else {
        Err(format!("未知的 AI 工具「{tool}」（只认 {}）", LIST_TOOLS.join(" / ")))
    }
}

/// 把 id 从一份偏好里「移出列表」（立墓碑 + 清掉加回记录）。
fn hide_in(slot: &mut ToolListPrefs, id: &str) {
    slot.shown.retain(|s| s != id); // 加回来过又移除 → 名单要对得上，否则下次还会冒出来
    if !slot.hidden.iter().any(|h| h == id) {
        slot.hidden.push(id.to_string());
    }
}

/// 把 id 加回一份偏好的列表（清墓碑；默认不摆出来的那几个还要记一笔「用户要它」）。
fn show_in(slot: &mut ToolListPrefs, id: &str) {
    slot.hidden.retain(|h| h != id);
    if SECONDARY_BUILTINS.contains(&id) && !slot.shown.iter().any(|s| s == id) {
        slot.shown.push(id.to_string());
    }
}

fn prefs_path() -> PathBuf {
    config_home().join(".uking").join("provider-prefs.json")
}

/// 读列表偏好（不存在/坏了都返回默认，绝不让列表崩）。
fn read_prefs() -> ListPrefs {
    std::fs::read_to_string(prefs_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// 写列表偏好（原子写：temp + rename）。
fn write_prefs(p: &ListPrefs) -> Result<(), String> {
    let path = prefs_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建 .uking 目录失败: {e}"))?;
    }
    let tmp = path.with_extension("json.tmp");
    let body = serde_json::to_string_pretty(p).map_err(|e| format!("序列化失败: {e}"))?;
    std::fs::write(&tmp, body).map_err(|e| format!("写 provider-prefs.json 失败: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("替换 provider-prefs.json 失败: {e}"))
}

/// 从某个 AI（或全部 AI）的列表里移除一个 provider。**幂等**（已经不在列表里也返回 Ok）。
///
/// - `tool = Some("claude")` → **只从 Claude Code 那份列表里拿走**，其余三个 AI 一字不动；
///   自定义供应商的定义和 Key 也原样留着（别的 AI 还在用它）。这是界面上垃圾桶按钮走的路。
/// - `tool = None` → 从**所有** AI 的列表里拿走；自定义的连定义带 Key 一起删（界面上的
///   「彻底删除」，以及 CLI / MCP / AI 没指定工具时的语义 —— 「从我的列表里删掉」）。
///
/// 注意它**只动 U-King 自己的列表**，不碰客户机器上任何 AI 工具的配置文件 —— 移除虾盘云
/// 不等于把 Claude Code 还原成官方。这两件事故意分开：替用户改他机器上已配好的东西，
/// 正是这次要根治的毛病。界面上会把这句话说给用户听（Manager.tsx 的确认框）。
pub fn remove_provider_for(tool: Option<&str>, id: &str) -> Result<(), String> {
    let id = id.trim();
    if id.is_empty() {
        return Err("provider id 不能为空".into());
    }
    // 「官方直连（还原）」是唯一的例外：它不是一个供应商，是「不用任何第三方」的**出口**。
    // 删掉它，用户就失去了把 Claude/Codex 还原成官方登录的入口 —— 那恰恰是「不抢登录态」
    // 这条原则的兜底。托盘菜单也无条件挂着它（tray.rs::switchable_providers），
    // 这里拒绝删除，两边才对得上（否则 CLI 删了、托盘还挂着，就是报告和世界不一致）。
    if id == "official" {
        return Err("「官方直连（还原）」不能移除 —— 它是还原成官方登录的出口，删了就没退路了".into());
    }
    if let Some(tg) = tool {
        check_tool(tg)?;
        if !all_providers().iter().any(|p| p.id == id) {
            return Ok(()); // 本来就没这个东西 —— 幂等，不报错
        }
        let mut prefs = read_prefs();
        hide_in(tool_slot(&mut prefs, tg), id);
        return write_prefs(&prefs);
    }
    // 全局：全局墓碑 + 每个已分家的工具也跟上，否则「彻底删除」在某个 AI 的页面里还留着一行。
    let mut prefs = read_prefs();
    let mut global = ToolListPrefs { hidden: prefs.hidden.clone(), shown: prefs.shown.clone(), order: vec![] };
    hide_in(&mut global, id);
    prefs.hidden = global.hidden;
    prefs.shown = global.shown;
    for slot in prefs.tools.values_mut() {
        hide_in(slot, id);
    }
    write_prefs(&prefs)?;
    // 自定义：定义连同 Key 一起删（这一步只有全局路径才做）。
    if read_custom_providers().iter().any(|p| p.id == id) {
        delete_custom_provider(id)?;
    }
    Ok(())
}


/// 把一个被移除的 provider 加回某个 AI（或全部 AI）的列表（「一键添加虾盘云」走这里）。**幂等**。
///
/// 只认**定义还在**的 id：内置永远在；自定义只要没被「彻底删除」就能加回来（它被移出某个
/// AI 的列表时定义并没有丢）。彻底删过的自定义就是没了，只能重新填一次 —— 用户自己的东西，
/// 我们不留副本。
pub fn restore_provider_for(tool: Option<&str>, id: &str) -> Result<(), String> {
    let id = id.trim();
    if !all_providers().iter().any(|p| p.id == id) {
        return Err(format!("「{id}」的定义已经不在了 —— 自定义供应商彻底删除后需要重新添加"));
    }
    let mut prefs = read_prefs();
    if let Some(tg) = tool {
        check_tool(tg)?;
        show_in(tool_slot(&mut prefs, tg), id);
        return write_prefs(&prefs);
    }
    let mut global = ToolListPrefs { hidden: prefs.hidden.clone(), shown: prefs.shown.clone(), order: vec![] };
    show_in(&mut global, id);
    prefs.hidden = global.hidden;
    prefs.shown = global.shown;
    for slot in prefs.tools.values_mut() {
        show_in(slot, id);
    }
    write_prefs(&prefs)
}


/// 保存某个 AI（或全局）的显示顺序（第一位 = 首选）。传进来的 id 里不认识的会被忽略，缺的排在后面。
pub fn set_provider_order_for(tool: Option<&str>, ids: Vec<String>) -> Result<(), String> {
    let known: Vec<String> = all_providers().into_iter().map(|p| p.id).collect();
    let order: Vec<String> = ids.into_iter().filter(|i| known.contains(i)).collect();
    let mut prefs = read_prefs();
    match tool {
        Some(tg) => {
            check_tool(tg)?;
            tool_slot(&mut prefs, tg).order = order;
        }
        None => prefs.order = order,
    }
    write_prefs(&prefs)
}


/// 某个 AI（或全局）里被用户**显式移除**过的 id（列表底部那行低调的「已移除：+ 虾盘云」用它）。
///
/// 注意跟 [`addable_for`] 的分工：这个只答「用户亲手删过谁」（所以默认状态是空的，那行小字
/// 不会无缘无故出现）；那个答「现在还能加谁」（含从没摆出来过的 SECONDARY_BUILTINS）。
/// 定义已经没了的（彻底删掉的自定义）不返回 —— 列出一个点了会报错的按钮不如不列。
pub fn hidden_ids_for(tool: Option<&str>) -> Vec<String> {
    let known: Vec<String> = all_providers().into_iter().map(|p| p.id).collect();
    view_prefs(&read_prefs(), tool)
        .hidden
        .into_iter()
        .filter(|h| known.contains(h))
        .collect()
}


/// 某个 AI（或全局）当前**不在列表里、但可以一键加回来**的供应商（带完整定义，前端直接拿
/// name/summary 画卡片）。内置和被移出这个 AI 的自定义都算 —— 后者的定义还在，能加回来。
///
/// 用途是「添加供应商」弹窗顶部那一排 —— 用户点开「添加」时才出现，等于：想要就随时拿得到，
/// 不想要就一眼都看不见。这是虾盘云被删掉之后**唯一**的常规回归入口（另一条是底部「已移除」
/// 那行，只在亲手删过之后出现）。**不做任何自动补种** —— 只回答「能加谁」，加不加是用户点的。
pub fn addable_for(tool: Option<&str>) -> Vec<ProviderPreset> {
    let listed: Vec<String> = list_providers_for(tool).into_iter().map(|p| p.id).collect();
    all_providers()
        .into_iter()
        .filter(|p| !listed.contains(&p.id))
        .collect()
}


// ============================================================
// 路径（支持 UKING_TEST_HOME 沙箱）
// ============================================================

fn config_home() -> PathBuf {
    if let Ok(t) = std::env::var("UKING_TEST_HOME") {
        if !t.is_empty() {
            return PathBuf::from(t);
        }
    }
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(home)
}

fn claude_settings_path() -> PathBuf {
    config_home().join(".claude").join("settings.json")
}

fn codex_dir() -> PathBuf {
    config_home().join(".codex")
}

/// DSH 的真实 home。正常是 `~/.dsh`；用户显式设了 `DSH_HOME` 就必须跟它走。
///
/// 沙箱测试优先级更高，否则开发机 shell 里残留的 `DSH_HOME` 会让 selfcheck
/// 越过 `UKING_TEST_HOME` 去改真配置。
fn dsh_dir() -> PathBuf {
    if std::env::var("UKING_TEST_HOME").map(|v| !v.is_empty()).unwrap_or(false) {
        return config_home().join(".dsh");
    }
    std::env::var("DSH_HOME")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| config_home().join(".dsh"))
}

/// 原子写文件:写到同目录临时文件再 rename 覆盖目标。
/// 防"写到一半进程崩/断电 → 配置半截损坏"(对齐 cc-switch 的 atomic_write，
/// 这是它"稳"的地基)。rename 在同一文件系统上是原子操作。
/// Windows 的 rename 不能覆盖已存在文件 → 先删目标再 rename。
fn atomic_write(path: &PathBuf, data: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建目录失败: {e}"))?;
    }
    // 临时名带 pid + 纳秒，避免并发/多实例撞名（无 rand 依赖，用时间戳）
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    let fname = path.file_name().and_then(|f| f.to_str()).unwrap_or("cfg");
    let tmp = path.with_file_name(format!(".{fname}.uking-tmp.{pid}.{stamp}"));
    std::fs::write(&tmp, data).map_err(|e| format!("写临时文件失败: {e}"))?;
    #[cfg(windows)]
    {
        if path.exists() {
            let _ = std::fs::remove_file(path);
        }
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp); // 失败别留垃圾
        return Err(format!("替换目标文件失败: {e}"));
    }
    // 写完回读校验（write-then-verify）。
    //
    // 教训来自 pc-*** / Issue #222 #223 #318 #319 #323：我们把 pip.ini 写坏了（UTF-8 vs cp936），
    // 却因为「写完就当成功」而整整一周没人发现 —— 日志上一路印着「已装好并通过自检」，
    // 那句「自检通过」说的是**写配置之前**的事实。**凡是我们写进客户机的东西，写完必须回读一次。**
    // 这里是 providers.rs 全部配置写入（Claude settings.json / Codex config.toml + auth.json /
    // ClawX providers / Hermes config.yaml + .env / 我们自己的 providers.json…）的唯一收口，
    // 加在这一处即全覆盖。
    //
    // 它挡的是真实发生过的故障模式：杀软/同步盘在我们 rename 之后又改写或截断文件
    // （便携 Python 那次「校验时 38116958 字节、解压时 16384 字节」就是这么来的）、
    // 磁盘写满、别的程序把内存副本刷回来盖掉我们写的。配置文件都是 KB 级，回读成本可忽略。
    match std::fs::read(path) {
        Ok(back) if back == data => Ok(()),
        Ok(back) => Err(format!(
            "配置写完后回读对不上（写入 {} 字节、读回 {} 字节）：{}。\
             多为杀毒软件/同步盘在我们写完后又动了它，或另一个程序把自己的内存副本刷了回来 —— \
             请把该目录加入杀软信任区，并确认相关程序已退出后重试。",
            data.len(),
            back.len(),
            path.display()
        )),
        Err(e) => Err(format!("配置写完后读不回来：{}：{e}", path.display())),
    }
}

/// 保留多少份历史备份（cc-switch 默认 10；U 盘版稍收敛到 5，够回滚又不占空间）。
const BACKUP_RETAIN: usize = 5;

/// 首次接管前备份「最原始配置」一次（`*.uking-bak`，已有则不覆盖 → 永远保住接管前那一份）。
/// 同时再追加一份**带时间戳的轮换备份**（`*.uking-bak.<ts>`），保留最近 BACKUP_RETAIN 份，
/// 这样用户多次切换后仍能回到任意较近的历史态（对齐 cc-switch 的多份轮换，旧实现只有单份会被首份锁死）。
fn backup_once(path: &PathBuf) {
    if !path.exists() {
        return;
    }
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("cfg");
    // ① 原始锚点备份：只在第一次接管时写，之后永不覆盖
    let anchor = path.with_extension(format!("{ext}.uking-bak"));
    if !anchor.exists() {
        let _ = std::fs::copy(path, &anchor);
    }
    // ② 轮换备份：每次都追加一份时间戳副本，再清理到最多 BACKUP_RETAIN 份
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let rolling = path.with_extension(format!("{ext}.uking-bak.{stamp}"));
    if !rolling.exists() {
        let _ = std::fs::copy(path, &rolling);
    }
    prune_rolling_backups(path, ext);
}

/// 清理某文件的轮换备份，按时间戳保留最近 BACKUP_RETAIN 份（不动原始锚点 `*.uking-bak`）。
fn prune_rolling_backups(path: &PathBuf, ext: &str) {
    let Some(dir) = path.parent() else { return };
    let Some(stem) = path.file_name().and_then(|f| f.to_str()) else { return };
    let prefix = format!("{stem}.uking-bak."); // 注意带点，排除锚点 *.uking-bak 本身
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut rolls: Vec<(u64, PathBuf)> = rd
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_str()?.to_string();
            let ts: u64 = name.strip_prefix(&prefix)?.parse().ok()?;
            let _ = ext; // 后缀已包含在 prefix 里
            Some((ts, e.path()))
        })
        .collect();
    if rolls.len() <= BACKUP_RETAIN {
        return;
    }
    rolls.sort_by_key(|(ts, _)| *ts); // 旧→新
    let drop = rolls.len() - BACKUP_RETAIN;
    for (_, p) in rolls.into_iter().take(drop) {
        let _ = std::fs::remove_file(p);
    }
}

// ============================================================
// 应用驱动
// ============================================================

/// U-King 在 settings.json env 里管理的键。
const MANAGED_ENV_KEYS: &[&str] = &[
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_MODEL",
    "ANTHROPIC_SMALL_FAST_MODEL",
    "API_TIMEOUT_MS",
    // 长会话省钱双键（2026-08-25 起，见 apply_claude_to 内注释）。进管理清单意味着
    // 「还原官方」时会把它们一并摘掉 —— 官方 Claude 模型有自己的压缩阈值，不该残留我们的。
    "CLAUDE_CODE_AUTO_COMPACT_WINDOW",
    "CLAUDE_CODE_MAX_CONTEXT_TOKENS",
];

/// 虾盘云 DeepSeek 族模型的「自动压缩计算窗口」默认值（token 数）。
///
/// 背景：deepseek-v4-flash 是 1M 窗口，Claude Code 默认按整个窗口的约 95% 才触发
/// 自动压缩 —— 客户会话滚到几十万 token 都不自知（2026-08-25 实测案例：8.5h 会话
/// 102 万 token、¥92/天）。把压缩计算窗口钉到 200K，让自动压缩提前动手，
/// 静态测算同等工作量省约 19%，叠加轮次治理更多。
pub const DEEPSEEK_AUTO_COMPACT_WINDOW: &str = "200000";

/// 端点是不是虾盘云自家域（host **后缀精确匹配**，fail-closed）。
///
/// 判定史（防回退）：
/// - opus P1：子串匹配时代 `fake-u-claw.org.evil.com` 能命中；光取「// 后第一段」还不够——
///   `https://u-claw.org:443@evil.com` 的真主机是 `evil.com`（@ 前是 user-info），必须先剥认证段。
/// - opus + sol 双会审（2026-08-27）：① 只认 http/https scheme 且 `://` 必须在开头——
///   `evil.com/r?u=https://api.u-claw.org`、`https:evil.com://api.u-claw.org` 这类假 scheme 壳
///   会把 query/数组里的 URL 当 authority；② http/https 是 WHATWG special scheme，反斜杠与斜杠
///   等价、同样截断 authority（`evil.com\@api.u-claw.org` 真 host 是 evil.com）；③ host 段 LDH
///   字符集闸 + C0 控制字符 + 非数字端口 + `[]` 开头一律拒（自家域是 DNS 名，不可能是 IPv6
///   字面量），未知形状 fail-closed，绝不把解析不出的怪串认成自家。
/// `pub(crate)`：lib.rs / uuswitch.rs 的「端点是不是虾盘云」判定也走这一份（避免
/// contains("u-claw.org") 在伪域/fake 域上误命中——见 23b6bbd 修 host 后缀匹配的
/// 同一病灶）。私有就编不过——见 working copy 引入 4 处调用时漏改可见性的那次。
pub(crate) fn is_xiapan_endpoint(base: &str) -> bool {
    let input = base.trim();
    // 只认「scheme://」开头的绝对 URL；裸串/协议相对 URL 都不是合法绝对 URL → 拒。
    let Some((scheme, rest)) = input.split_once("://") else {
        return false;
    };
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return false;
    }
    // 只看 authority 段（到第一个 / ? # \ 为止；反斜杠在 special scheme 里等价斜杠）
    let authority = rest.split(['/', '?', '#', '\\']).next().unwrap_or("");
    // C0 控制字符（含 tab/CR/LF/空格）：fail-closed，也顺带封掉 header 注入面
    if authority.is_empty() || authority.bytes().any(|b| b <= b' ' || b == 0x7f) {
        return false;
    }
    // 最后一个字面量 '@' 是 user-info / host 分界（WHATWG authority state 语义）
    let hostport = authority
        .rsplit_once('@')
        .map(|(_, h)| h)
        .unwrap_or(authority);
    // 自家域是 DNS 域名，不可能是 IPv6 字面量；[] 开头（剥括号也没验合法性）一律拒
    if hostport.starts_with('[') {
        return false;
    }
    // 端口只认纯数字且 ≤ u16；空端口非法
    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) if !h.contains(':') => (h, Some(p)),
        Some(_) => return false,
        None => (hostport, None),
    };
    if port.is_some_and(|p| p.is_empty() || p.parse::<u16>().is_err()) {
        return false;
    }
    // host 段 LDH 字符集闸（非 ASCII / % / 尖括号等怪字符全拒，fail-closed）
    if host.is_empty() || !host.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.') {
        return false;
    }
    // DNS absolute name 的末尾点不改变归属
    let host = host.strip_suffix('.').unwrap_or(host).to_ascii_lowercase();
    !host.is_empty()
        && (host == "u-claw.org"
            || host.ends_with(".u-claw.org")
            || host == "u-claw.org.cn"
            || host.ends_with(".u-claw.org.cn"))
}

#[derive(Debug, Clone, Serialize)]
pub struct ApplyResult {
    pub claude: Option<String>,
    pub codex: Option<String>,
    pub clawx: Option<String>,
    pub hermes: Option<String>,
    /// DeepSeek Harness（Web / terminal 共用 `$DSH_HOME`）。
    pub dsh: Option<String>,
    /// Qwen Code（2026-08-03 上架）。**新增字段而不是复用旧的** —— 前端按名字读，
    /// 加字段是纯增量，老前端读不到它只是不显示，不会崩。
    pub qwen: Option<String>,
    /// Crush（2026-08-03 上架）。
    pub crush: Option<String>,
    /// OpenCode（2026-08-03 上架，**仅 TUI 可用**，见 apply_opencode 注释）。
    pub opencode: Option<String>,
    /// pi（2026-08-03 上架）。四条门槛实测全过，见 apply_pi 注释。
    pub pi: Option<String>,
    /// Cline（2026-08-29 上架）。纯增量字段，老前端读不到只是不显示。
    pub cline: Option<String>,
}

// ============================================================
// 当前驱动记录（对齐 cc-switch 的 is_current）
// ------------------------------------------------------------
// cc-switch 把「每个工具当前选了谁」**显式记在自己的库里**，回显直接读它，
// 绝不靠读活动配置文件反推。U-King 早期偷懒成「读文件猜」，Hermes 那条猜错了
// （有 model 就当虾盘云）→ 切官方后仍显示使用中。这里补上 cc-switch 的核心：
// 一个 `~/.uking/active-drivers.json`（纯 std，不引 DB/crate），切一次记一笔，
// 回显以它为准。official = 还原官方。
// ============================================================

fn active_drivers_path() -> PathBuf {
    config_home().join(".uking").join("active-drivers.json")
}

/// 读「各工具当前生效的 provider id」记录（读不到/损坏 = 空表）。
fn load_active_drivers() -> serde_json::Map<String, Value> {
    std::fs::read_to_string(active_drivers_path())
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

/// 记一笔「tool 切到了 provider_id」（official=还原官方）。原子写、自动建目录、失败不致命。
fn record_active_driver(tool: &str, provider_id: &str) {
    let path = active_drivers_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let mut map = load_active_drivers();
    map.insert(tool.to_string(), Value::String(provider_id.to_string()));
    if let Ok(s) = serde_json::to_string_pretty(&Value::Object(map)) {
        let _ = atomic_write(&path, s.as_bytes());
    }
}

/// 从 base URL 反推 provider id（仅作「无显式记录」时的兜底/老安装引导，修正 Hermes 老 bug）。
fn id_from_base(base: Option<&str>) -> Option<String> {
    let b = base.unwrap_or("");
    if is_xiapan_endpoint(b) {
        Some("xiapan".into())
    } else if b.contains("deepseek") {
        Some("deepseek".into())
    } else if b.contains("bigmodel") {
        Some("glm".into())
    } else if b.contains("moonshot") {
        Some("kimi".into())
    } else {
        None
    }
}

/// 某个供应商的 OpenAI 端点。给组合根 `lib.rs` 起翻译桥用
/// （功能模块之间不互相 import，桥不认识 provider、provider 也不认识桥）。
pub fn openai_base_of(provider_id: &str) -> Result<String, String> {
    let p = preset(provider_id)?;
    let b = p.openai_base.trim().to_string();
    if b.is_empty() {
        return Err(format!("{} 没填 OpenAI 端点，起不了翻译桥", p.name));
    }
    Ok(b)
}

/// ★ 走**本地翻译桥**把某个供应商配给 Claude Code。**桥必须已经起好**（不归这儿管）。
///
/// 返回写进 `ANTHROPIC_BASE_URL` 的值，给调用方回显。
///
/// 🔴 **Key 绝不回退到设备 Key**：这条路专给「客户自带的中转」用，
/// 悄悄把我们的虾盘云设备 Key 发去第三方端点 = 把自家 Key 泄给别人家服务器。
/// 没 Key 就明说没 Key。
pub fn apply_claude_bridged(
    provider_id: &str,
    api_key: &str,
    model_override: Option<&str>,
    bridge_base: &str,
) -> Result<String, String> {
    let p = preset(provider_id)?;
    if supports_claude_code(&p) {
        return Err(format!("{} 本身就支持 Claude Code，直接切就行，不用绕本地桥", p.name));
    }
    let key = if !api_key.trim().is_empty() {
        api_key.trim().to_string()
    } else if !p.api_key.trim().is_empty() {
        p.api_key.trim().to_string()
    } else {
        return Err(format!("{} 还没填 API Key", p.name));
    };
    apply_claude_via_bridge(&p, &key, model_override, bridge_base)?;
    record_active_driver("claude", provider_id);
    Ok(bridge_base.to_string())
}

/// 把驱动应用到 Claude Code / Codex / ClawX（targets 里有谁就改谁）。
pub fn apply_provider(
    provider_id: &str,
    api_key: &str,
    model_override: Option<&str>,
    targets: &[String],
) -> Result<ApplyResult, String> {
    let p = preset(provider_id)?;
    let mut r = ApplyResult {
        claude: None,
        codex: None,
        clawx: None,
        hermes: None,
        dsh: None,
        qwen: None,
        crush: None,
        opencode: None,
        pi: None,
        cline: None,
    };

    for t in targets {
        match t.as_str() {
            "claude" => {
                if p.id == "official" {
                    reset_claude()?;
                    r.claude = Some("已还原官方配置".into());
                    record_active_driver("claude", provider_id);
                } else if targets.len() > 1 && !supports_claude_code(&p) {
                    // 纯 OpenAI 兼容的供应商**驱动不了 Claude Code**（它只说 Anthropic Messages
                    // 协议）。这是能力不匹配，不是失败 —— 多目标时跳过它，别让本来能配好的
                    // Codex / ClawX 跟着一起陪葬（跟 Hermes / qwen / pi / crush「没装就跳过」同一条理由）。
                    //
                    // 真 bug（issue #359 火山方舟豆包、#322 一个叫 Claude 的中转）：装机向导
                    // targetsFromInstalled() 会一次传 claude+codex，客户挑了个只有 OpenAI 端点的
                    // 供应商 → 这里 `?` 抛错 → 整次写入失败 → 向导 retryDriver 打转，
                    // 客户卡在首装最后一米，而 Codex 那半边其实完全能用。
                    //
                    // **故意不 record_active_driver**：没配上就不能记成「在用」，否则回显撒谎。
                    // 出路已经有了，但**故意不在这儿自动开**：桥跟着 U-King 活，
                    // 悄悄替客户开了，等他哪天关掉 U-King，Claude Code 会在他不知情的
                    // 时候连不上 —— 失败得更晚、更莫名，比现在直说「配不了」还糟。
                    // 所以这儿只告诉他有这条路，开不开他自己点（AI 设置页）。
                    r.claude = Some(format!(
                        "已跳过 Claude Code：{} 只提供 OpenAI 兼容接口，而 Claude Code 只认 Anthropic 接口。\
                         想用它驱动 Claude Code，可以在「AI 设置」里给它开本地翻译桥（需 U-King 保持运行）",
                        p.name
                    ));
                } else {
                    apply_claude(&p, api_key, model_override)?;
                    r.claude = Some(format!("已切到 {}（{}）", p.name, effective_model(&p, model_override)));
                    record_active_driver("claude", provider_id);
                }
            }
            "codex" => {
                if p.id == "official" {
                    reset_codex()?;
                    r.codex = Some("已还原官方配置".into());
                } else {
                    apply_codex(&p, api_key, model_override)?;
                    r.codex = Some(format!("已切到 {}（{}）", p.name, effective_codex_model(&p, model_override)));
                }
                record_active_driver("codex", provider_id);
            }
            "clawx" => {
                if p.id == "official" {
                    reset_clawx()?;
                    r.clawx = Some("已移除 ClawX 的虾盘云配置".into());
                } else {
                    let model = effective_model(&p, model_override);
                    apply_clawx(&p, api_key, &model)?;
                    r.clawx = Some(format!("已把 ClawX 模型切到 {}（{}）", p.name, model));
                }
                record_active_driver("clawx", provider_id);
            }
            "hermes" => {
                if p.id == "official" {
                    reset_hermes()?;
                    r.hermes = Some("已移除 Hermes 的虾盘云配置".into());
                    record_active_driver("hermes", provider_id);
                } else {
                    let model = effective_model(&p, model_override);
                    // Hermes 未装时静默跳过（~/.hermes 不存在），不让整次切换失败
                    match apply_hermes(&p, api_key, &model) {
                        Ok(()) => {
                            r.hermes = Some(format!("已把 Hermes 模型切到 {}（{}）", p.name, model));
                            record_active_driver("hermes", provider_id);
                        }
                        Err(e) if e.contains("未检测到 Hermes") => {}
                        Err(e) => return Err(e),
                    }
                }
            }
            "dsh" => {
                if p.id == "official" {
                    reset_dsh()?;
                    r.dsh = Some("已移除 DSH 的 U-King 模型配置".into());
                } else {
                    let model = effective_model(&p, model_override);
                    apply_dsh(&p, api_key, &model)?;
                    r.dsh = Some(format!("已把 DSH Web / 终端切到 {}（{}）", p.name, model));
                }
                record_active_driver("dsh", provider_id);
            }
            // Qwen Code / Crush（2026-08-03 上架）。**没装就静默跳过**，跟 Hermes 同样处理：
            // 「一键配好全部」会把所有 target 都传进来，为一个没装的工具让整次切换失败，
            // 客户看到的就是「点了一下，什么都没配上」。
            "qwen" => {
                if p.id == "official" {
                    reset_qwen()?;
                    r.qwen = Some("已移除 Qwen Code 的虾盘云配置".into());
                    record_active_driver("qwen", provider_id);
                } else if crate::installer::tool_installed("qwen") {
                    let model = effective_model(&p, model_override);
                    apply_qwen(&p, api_key, &model)?;
                    r.qwen = Some(format!("已把 Qwen Code 切到 {}（{}）", p.name, model));
                    record_active_driver("qwen", provider_id);
                }
            }
            "pi" => {
                if p.id == "official" {
                    reset_pi()?;
                    r.pi = Some("已移除 pi 的虾盘云配置".into());
                    record_active_driver("pi", provider_id);
                } else if crate::installer::tool_installed("pi") {
                    let model = effective_model(&p, model_override);
                    apply_pi(&p, api_key, &model)?;
                    r.pi = Some(format!("已把 pi 切到 {}（{}）", p.name, model));
                    record_active_driver("pi", provider_id);
                }
            }
            "crush" => {
                if p.id == "official" {
                    reset_crush()?;
                    r.crush = Some("已移除 Crush 的虾盘云配置".into());
                    record_active_driver("crush", provider_id);
                } else if crate::installer::tool_installed("crush") {
                    let model = effective_model(&p, model_override);
                    apply_crush(&p, api_key, &model)?;
                    r.crush = Some(format!("已把 Crush 切到 {}（{}）", p.name, model));
                    record_active_driver("crush", provider_id);
                }
            }
            "opencode" => {
                if p.id == "official" {
                    reset_opencode()?;
                    r.opencode = Some("已移除 OpenCode 的虾盘云配置".into());
                    record_active_driver("opencode", provider_id);
                } else if crate::installer::tool_installed("opencode") {
                    let model = effective_model(&p, model_override);
                    apply_opencode(&p, api_key, &model)?;
                    r.opencode = Some(format!("已把 OpenCode 切到 {}（{}）", p.name, model));
                    record_active_driver("opencode", provider_id);
                }
            }
            // Cline（2026-08-29 上架）：同 pi/opencode 口径 —— 未装静默跳过，
            // official 走 reset（回滚备份 / 只删我们的槽位）。
            "cline" => {
                if p.id == "official" {
                    reset_cline()?;
                    r.cline = Some("已移除 Cline 的虾盘云配置".into());
                    record_active_driver("cline", provider_id);
                } else if crate::installer::tool_installed("cline") {
                    let model = effective_model(&p, model_override);
                    apply_cline(&p, api_key, &model)?;
                    r.cline = Some(format!("已把 Cline 切到 {}（{}）", p.name, model));
                    record_active_driver("cline", provider_id);
                }
            }
            other => return Err(format!("未知目标 {other}")),
        }
    }
    Ok(r)
}

/// 「一键配好全部」结果：每个工具有没有配上 + 提示。
#[derive(Debug, Clone, Serialize, Default)]
pub struct ApplyAllResult {
    /// 实际写了配置的工具名（中文，给前端拼一句话）
    pub configured: Vec<String>,
    /// 探测到但跳过的工具 + 原因（如「未安装」），可空
    pub skipped: Vec<String>,
    /// 是否需要重启 ClawX 才能生效（写了 ClawX 配置时为 true）
    pub clawx_needs_restart: bool,
    /// ★ 配成功的**稳定工具 id**（`claude` / `pi` / `crush`…）。
    /// 上面两个是给人看的中文，会随文案改；聚合统计只能认 id，否则改一次文案历史数据就断。
    /// 组合根 lib.rs 拿它落 metrics，本模块不 import metrics（依赖方向）。
    pub configured_ids: Vec<String>,
    /// 跳过的稳定工具 id（原因仍在 `skipped` 的中文里）
    pub skipped_ids: Vec<String>,
}

/// **一键配好全部** —— 小白主路径。
///
/// 后端自己探测装了哪些工具（不信前端的 installed 列表，那是 Bug A 真因：
/// ClawX 装完才下载，前端列表里没它，配置就没写进 clawx-providers.json），
/// 把指定 provider（默认虾盘云）的 Key 一次性写进**所有探测到的**工具。
///
/// 探测口径：
///  - Claude Code：`claude` 命令在（installer::tool_installed）→ 写 settings.json
///  - Codex：`codex` 命令在 或 Codex 桌面版装了 → 写 config.toml
///  - ClawX：桌面版装了或已有配置目录 → 写 ClawX provider + OpenClaw agent 层
///  - Hermes：命令已装或已有配置目录 → 写 .env/config.yaml
///
/// 任一工具写失败不阻塞其它（best-effort），最后汇总告诉用户配了谁。
///
/// ## `only`：装没装是**事实**，配哪些是**意图**（0.9.84）
///
/// `only=None` = 配探到的全部（老行为，向后兼容）。给了名单就只配名单里的
/// （`claude` / `codex` / `clawx` / `hermes`）。
///
/// 这跟上面那句「不信前端的 installed 列表」不矛盾，两件事必须分开看：
///  - **哪些装了** = 客观事实，前端会看走眼（ClawX 装完才下载那个 Bug），永远后端自己探；
///  - **改哪些** = 用户意图，只有用户知道 —— 他可能就是不想让我们碰他的 Codex。
///
/// 名单只做减法：写进名单也得先通过后端的「真装了吗」探测，不会因为前端说装了就去写。
pub fn apply_xiapan_everywhere(
    provider_id: &str,
    api_key: &str,
    model_override: Option<&str>,
    only: Option<&[String]>,
) -> Result<ApplyAllResult, String> {
    let p = preset(provider_id)?;
    if api_key.trim().is_empty() {
        return Err("缺少 API Key（请先开通虾盘云内置 Key）".into());
    }
    // 用户没勾任何工具就直接拒 —— 静默成功却什么都没配，比报错更难查。
    if only.map(|o| o.is_empty()).unwrap_or(false) {
        return Err("没有选中任何工具".into());
    }
    let want = |tool: &str| only.map(|o| o.iter().any(|x| x == tool)).unwrap_or(true);
    let mut r = ApplyAllResult::default();

    // Claude Code
    if want("claude") && crate::installer::tool_installed("claude") {
        match apply_claude(&p, api_key, model_override) {
            Ok(()) => {
                r.configured.push("Claude Code".into());
                r.configured_ids.push("claude".into());
                record_active_driver("claude", provider_id);
            }
            Err(e) => {
                r.skipped.push(format!("Claude Code（{e}）"));
                r.skipped_ids.push("claude".into());
            }
        }
    }
    // Codex（CLI 或桌面版任一）。
    // 自动批量这条路要**让着客户已有的东西**：官方 ChatGPT 登录、或他自己写的 config.toml，
    // 一律跳过并如实报出来（不是静默跳过 —— 那会变成「点了没反应」）。
    // 在「Codex 工作站」里显式点切换不受这条限制，那是明确授权。
    if want("codex") && (crate::installer::tool_installed("codex") || crate::installer::codex_app_installed()) {
        match codex_auto_config_blocked() {
            Some(why) => {
                r.skipped.push(format!("Codex（{why}）"));
                r.skipped_ids.push("codex".into());
            }
            None => match apply_codex(&p, api_key, model_override) {
                Ok(()) => {
                    r.configured.push("Codex".into());
                    r.configured_ids.push("codex".into());
                    record_active_driver("codex", provider_id);
                }
                Err(e) => {
                    r.skipped.push(format!("Codex（{e}）"));
                    r.skipped_ids.push("codex".into());
                }
            },
        }
    }
    // ClawX / OpenClaw agent 层：这次必须进小白主路径。
    // apply_clawx 本身已做「只动 U-King 账号 + 需要重启生效」的幂等写入；如果只在进阶页可配，
    // 客户最常点的「一键接入虾盘云」就会漏掉 ClawX provider 和 OpenClaw agent key。
    if want("clawx") && clawx_configurable() {
        let model = effective_model(&p, model_override);
        match apply_clawx(&p, api_key, &model) {
            Ok(()) => {
                r.configured.push("ClawX / OpenClaw".into());
                r.configured_ids.push("clawx".into());
                r.clawx_needs_restart = true;
                record_active_driver("clawx", provider_id);
            }
            Err(e) => {
                r.skipped.push(format!("ClawX / OpenClaw（{e}）"));
                r.skipped_ids.push("clawx".into());
            }
        }
    }
    // Hermes（CLI / 网页版）—— 2026-06-27 重新并入小白主路径。
    // 当年（2026-06-20）把它和 ClawX 一起排除，是因为 apply_hermes 还坏着（.env 没无条件写、
    // 用了被删的内置 provider 名）。现两个根因都已修，且 pc-*** 真机实测 provider=custom +
    // 仅 OPENAI_*（.org.cn）→ HELLO_OK。Hermes 是 CLI 式、每次起进程都重读 .env/config.yaml，
    // **没有 ClawX 那种「Electron 内存配置副本要重启才生效」的坑** —— 所以「切了没反应」那条
    // 头号售后单的真主角是 ClawX、不是它，可安全自动配。每文件 backup_once 留底，可回滚。
    if want("hermes") && (hermes_dir().exists() || crate::installer::tool_installed("hermes")) {
        let model = effective_model(&p, model_override);
        match apply_hermes(&p, api_key, &model) {
            Ok(()) => {
                r.configured.push("Hermes".into());
                r.configured_ids.push("hermes".into());
                record_active_driver("hermes", provider_id);
            }
            // 目录刚好消失等竞态 → 跳过，不让整次「一键配好全部」失败
            Err(e) if e.contains("未检测到 Hermes") => {}
            Err(e) => {
                r.skipped.push(format!("Hermes（{e}）"));
                r.skipped_ids.push("hermes".into());
            }
        }
    }
    // DeepSeek Harness：Web / terminal 两个入口都读 `$DSH_HOME`，这里配一次即全部生效。
    if want("dsh") && crate::installer::tool_installed("dsh") {
        let model = effective_model(&p, model_override);
        match apply_dsh(&p, api_key, &model) {
            Ok(()) => {
                r.configured.push("DeepSeek Harness（Web / 终端）".into());
                r.configured_ids.push("dsh".into());
                record_active_driver("dsh", provider_id);
            }
            Err(e) => {
                r.skipped.push(format!("DeepSeek Harness（{e}）"));
                r.skipped_ids.push("dsh".into());
            }
        }
    }
    // ★ 后上架的 CLI（2026-08-03 补齐）。此前它们全都不在「一键配好全部」里 ——
    // 于是「装机向导装完 → 自动接虾盘云」这条主路径上，pi 装上了却没配驱动，
    // 客户敲 `pi` 直接撞 google 的交互式登录。pi 进了默认三件套后这就不是待办，是断链。
    // 顺手把 Qwen / Crush / OpenCode 一起接上：它们的 apply 早就写好了，只是没人调用。
    // 判据仍是**后端自探已装**（`tool_installed`），没装的静默跳过，同 Hermes 的处理。
    for (tool, label, apply) in [
        ("pi", "pi", apply_pi as fn(&ProviderPreset, &str, &str) -> Result<(), String>),
        ("qwen", "Qwen Code", apply_qwen),
        ("crush", "Crush", apply_crush),
        ("opencode", "OpenCode", apply_opencode),
        ("cline", "Cline", apply_cline),
    ] {
        if want(tool) && crate::installer::tool_installed(tool) {
            let model = effective_model(&p, model_override);
            match apply(&p, api_key, &model) {
                Ok(()) => {
                    r.configured.push(label.into());
                    r.configured_ids.push(tool.into());
                    record_active_driver(tool, provider_id);
                }
                Err(e) => {
                    r.skipped.push(format!("{label}（{e}）"));
                    r.skipped_ids.push(tool.into());
                }
            }
        }
    }
    // 一个都没探测到：把 Claude 配置先写好（装完即用），不让用户白点一次。
    // ⚠️ 但**用户点名要配哪些时不许这么做** —— 他只勾了 Codex，我们却回头写了他的
    // Claude 配置，那就是打着"贴心"的旗号动他没授权的东西，和这一轮要根治的是同一个毛病。
    if r.configured.is_empty() && want("claude") {
        if let Ok(()) = apply_claude(&p, api_key, model_override) {
            r.configured.push("Claude Code（预配置，装好即用）".into());
            r.configured_ids.push("claude".into());
            record_active_driver("claude", provider_id);
        }
    }
    Ok(r)
}

fn clawx_configurable() -> bool {
    if clawx_app_installed() {
        return true;
    }
    let path = clawx_providers_path();
    path.exists() || path.parent().map(|p| p.exists()).unwrap_or(false)
}

/// 启动时的 ClawX 例行检查（**只探测、只放行防火墙，绝不写用户配置**）。
///
/// 旧行为（已废弃，2026-06-17）：检测到 ClawX 没接虾盘云就后台静默覆盖
/// `clawx-providers.json`。问题同前端那两个自动入口——后台线程偷偷改用户配置，
/// 用户完全无感知就被切走，比前端更隐蔽。现改为 cc-switch 哲学：**绝不自动写**，
/// 是否接入由用户在前端主动点（前端读 `clawx_needs_xiapan()` 弹非侵入提示条引导）。
///
/// 这里只保留「防火墙放行」——那是网络放行（让 ClawX 能联网），不是改用户的模型/Key 配置，
/// 客户可能自己装的 ClawX 没经过装机预放行。幂等 + best-effort，没提权静默失败。
/// 修客户机上**已经被写坏**的 Codex 配置（Issue #364）。启动时后台跑一次。
///
/// 只改代码不够：0.9.92 及更早版本已经把 `wire_api = "chat"` 写进了一批客户的
/// `~/.codex/config.toml`。新版 Codex 见到这个值会**整份配置拒绝加载**，那些机器上
/// Codex 现在是完全起不动的（客户日志里每次都是 `code=1 total=0s events=0`）。
/// 他们不会知道「再切一次驱动就好了」—— 他们只会认为 Codex 坏了、或者 U-King 把它搞坏了。
///
/// 三条边界：
/// - **只动带 `managed by U-King` 标记的文件**，不是我们写的一个字节都不碰；
/// - 只改这一处键值 + 顺带把老的 `env_key` 换成新版认的 bearer（key 从我们自己写的
///   `auth.json` 里取），其余原样保留 —— 用户挂的 `[mcp_servers.*]` 必须活着；
/// - 拿不到 key 也要把 `wire_api` 修掉：**让 Codex 能启动**比让它能鉴权更紧急，
///   前者是全盘瘫痪，后者只是那个 provider 要重切一次。
pub fn heal_codex_wire_api() {
    let cfg = codex_dir().join("config.toml");
    let Ok(old) = std::fs::read_to_string(&cfg) else { return };
    if !old.contains("managed by U-King") || !old.contains(r#"wire_api = "chat""#) {
        return;
    }
    let mut fixed = old.replace(r#"wire_api = "chat""#, &format!(r#"wire_api = "{WIRE_API}""#));
    // 老的 chat 链路写的是 env_key，新版 Codex 对自定义 provider 不读 auth.json、
    // 只认环境变量 → 换成 experimental_bearer_token，把 key 直接内联。
    if fixed.contains(r#"env_key = "OPENAI_API_KEY""#) {
        if let Some(key) = std::fs::read_to_string(codex_dir().join("auth.json"))
            .ok()
            .and_then(|s| {
                s.split("\"OPENAI_API_KEY\"")
                    .nth(1)
                    .and_then(|rest| rest.split('"').nth(1))
                    .map(|k| k.to_string())
            })
            .filter(|k| !k.trim().is_empty())
        {
            fixed = fixed.replace(
                r#"env_key = "OPENAI_API_KEY""#,
                &format!(r#"experimental_bearer_token = "{key}""#),
            );
        }
    }
    match atomic_write(&cfg, fixed.as_bytes()) {
        Ok(()) => crate::ulog::write(
            "providers",
            "已修复 Codex 配置里失效的 wire_api=\"chat\"（新版 Codex 会因此整份配置加载失败）",
        ),
        Err(e) => crate::ulog::write("providers", &format!("修复 Codex wire_api 失败: {e}")),
    }
}

pub fn clawx_firewall_only() {
    if !clawx_app_installed() {
        return;
    }
    #[cfg(windows)]
    crate::tools::add_clawx_firewall_rule();
}

/// 探测：ClawX 已装、但 `clawx-providers.json` 还没被 U-King 接管（defaultProvider
/// 不在 `uking-*` 命名空间）→ 返回 true。供前端弹「检测到 ClawX，是否接入虾盘云?」提示条，
/// **不写任何配置**。未装 / 已接管 / 读不到文件（视为没接管）均按需返回。
pub fn clawx_needs_xiapan() -> bool {
    if !clawx_app_installed() {
        return false;
    }
    if let Ok(s) = std::fs::read_to_string(clawx_providers_path()) {
        if let Ok(v) = serde_json::from_str::<Value>(&s) {
            if v
                .get("defaultProvider")
                .and_then(Value::as_str)
                .is_some_and(is_managed_provider_id)
            {
                return false; // 已被我们接管
            }
        }
    }
    true
}

fn effective_model(p: &ProviderPreset, model_override: Option<&str>) -> String {
    model_override
        .filter(|m| !m.trim().is_empty())
        .map(|m| m.to_string())
        .unwrap_or_else(|| p.model.clone())
}

/// Codex 链路用的模型：override > 预设 codex_model > 预设 model。
fn effective_codex_model(p: &ProviderPreset, model_override: Option<&str>) -> String {
    model_override
        .filter(|m| !m.trim().is_empty())
        .map(|m| m.to_string())
        .unwrap_or_else(|| {
            if p.codex_model.is_empty() {
                p.model.clone()
            } else {
                p.codex_model.clone()
            }
        })
}

/// 这个供应商能不能驱动 Claude Code？
///
/// 判据只有一条：有没有 Anthropic 格式端点。Claude Code 只说 Anthropic Messages 协议，
/// 纯 OpenAI 兼容的中转（火山方舟、多数自建网关）给它是配不上的 —— 这是**能力**问题，
/// 不是 Key 或网络问题，重试多少次都一样。
///
/// 抽出来是为了让「配之前先判断」和 `apply_claude` 里「配的时候才发现」用同一份判据
/// （宪法第 8 条：同一事实存在几份就会漂移几份）。
pub fn supports_claude_code(p: &ProviderPreset) -> bool {
    p.anthropic_base.as_deref().map(|s| !s.trim().is_empty()).unwrap_or(false)
}

fn apply_claude(p: &ProviderPreset, key: &str, model_override: Option<&str>) -> Result<(), String> {
    apply_claude_to(p, key, model_override, None)
}

/// ★ 走**本地翻译桥**接 Claude Code：`ANTHROPIC_BASE_URL` 指向桥，其余键照旧。
///
/// 给「只有 OpenAI 端点」的供应商用（issue #359 / #322）。桥把 Anthropic Messages 翻成
/// chat/completions 再转发上去。
///
/// **桥的启停不归这儿管** —— 功能模块之间不互相 import（宪法第 12 条），
/// 由组合根 `lib.rs` 先把桥起起来、再把它的 base 传进来。这儿只负责写配置。
///
/// 🔴 调用方必须让客户知道：**桥是 U-King 的子进程，U-King 一退桥就没了**，
/// 那一刻 Claude Code 会连不上。别把这句藏在帮助里。
pub fn apply_claude_via_bridge(
    p: &ProviderPreset,
    key: &str,
    model_override: Option<&str>,
    bridge_base: &str,
) -> Result<(), String> {
    apply_claude_to(p, key, model_override, Some(bridge_base))
}

/// `base_override` 非空 = 走本地桥；否则用供应商自己的 Anthropic 端点。
fn apply_claude_to(
    p: &ProviderPreset,
    key: &str,
    model_override: Option<&str>,
    base_override: Option<&str>,
) -> Result<(), String> {
    let base = match base_override {
        Some(b) => b.to_string(),
        None => p
            .anthropic_base
            .clone()
            .ok_or_else(|| format!("{} 不支持 Claude Code 接入", p.name))?,
    };
    // 防御：base 必须是真实 http(s) URL。空串/非法值会让 claude 启动报
    // 「"" cannot be parsed as a URL」(历史 bug 写坏过)。这里硬挡，绝不写坏值。
    let base = base.trim().to_string();
    if !(base.starts_with("http://") || base.starts_with("https://")) {
        return Err(format!("{} 的接入地址非法（{base:?}）", p.name));
    }
    if key.trim().is_empty() {
        return Err("API Key 不能为空".into());
    }
    let path = claude_settings_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建 .claude 目录失败: {e}"))?;
    }
    let mut root: Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({}));
    backup_once(&path);

    let model = effective_model(p, model_override);
    let env = root
        .as_object_mut()
        .ok_or("settings.json 顶层不是对象")?
        .entry("env")
        .or_insert_with(|| json!({}));
    let env = env.as_object_mut().ok_or("settings.json 的 env 不是对象")?;
    // ── 长会话省钱：自动压缩窗口（2026-08-25）──
    // GPT-5.6-sol 会审定案（v3 评审 C/B 条）：**只对虾盘云 DeepSeek 族默认 200K，
    // 其余模型一律跟随 Claude Code 官方默认**。用户已自己配过这两个键的，让路不覆盖。
    // CLAUDE_CODE_AUTO_COMPACT_WINDOW = 压缩计算用的「有效容量」（不是到值必压）；
    // CLAUDE_CODE_MAX_CONTEXT_TOKENS = 网关/自定义模型 ID 场景下纠正窗口认知。
    let model_lower = model.to_lowercase();
    let is_xiapan_deepseek =
        is_xiapan_endpoint(&base) && (model_lower.contains("deepseek") || model_lower.contains("mimo"));
    // 标记写序（sol 复审 P0 + opus P1-4 双会审契约）：
    //   注入方向：先标记落盘成功 → 才许写 settings（标记写失败 Err 早退，settings 不写）；
    //   归还方向（切到非 deepseek）：restore 只改内存 → settings 原子写成功 → 才消费标记。
    //   两方向都保证「settings 侧失败时标记状态与磁盘一致、重试可收敛」。
    let mut defer_prov: Option<serde_json::Map<String, Value>> = None;
    if is_xiapan_deepseek {
        // 归属追踪（sol 复审实抓：仅凭值=="200000"分不清「我们注入」还是「用户恰好自配同值」）：
        // 注入前把每个键的原始状态记进 ~/.uking/claude-env-provenance.json，
        // 还原时按标记精确归还——有标记的键才动，标记记的是 null（注入前不存在）才删。
        let mut prov = read_window_provenance();
        for k in ["CLAUDE_CODE_AUTO_COMPACT_WINDOW", "CLAUDE_CODE_MAX_CONTEXT_TOKENS"] {
            if !env.contains_key(k) {
                env.insert(k.into(), json!(DEEPSEEK_AUTO_COMPACT_WINDOW));
                prov.entry(k).or_insert_with(|| serde_json::Value::Null);
            }
        }
        // 注入方向：先标记后 settings —— settings 写失败时标记仍在，重试复用
        write_window_provenance(&prov)?;
    } else {
        // opus 会审 P1-4（2026-08-27 实读代码）：虾盘云内从 deepseek 切到 kimi/glm 等
        // 非 deepseek 系，或切到任何非虾盘云 deepseek 路由——窗口键必须按归属标记归还/摘除，
        // 不许给 256K/1M 窗口的模型戴 200K 帽子（设计不变量「官方模型零注入」要在每次
        // apply 成立，不只首次）。归还方向：标记消费延后到 settings 落盘之后（函数尾）。
        let mut prov = read_window_provenance();
        restore_window_keys(env, &mut prov);
        defer_prov = Some(prov);
    }
    env.insert("ANTHROPIC_BASE_URL".into(), json!(base));
    env.insert("ANTHROPIC_AUTH_TOKEN".into(), json!(key));
    env.insert("ANTHROPIC_MODEL".into(), json!(model));
    env.insert("ANTHROPIC_SMALL_FAST_MODEL".into(), json!(p.small_model));
    env.insert("API_TIMEOUT_MS".into(), json!("600000"));

    atomic_write(&path, serde_json::to_string_pretty(&root).unwrap().as_bytes())
        .map_err(|e| format!("写 settings.json 失败: {e}"))?;
    // 归还方向延迟消费：settings 落盘成功才消费标记（失败则标记仍在、重试可精确回收）
    if let Some(prov) = defer_prov {
        write_window_provenance(&prov)?;
    }
    Ok(())
}

/// 按归属标记把窗口双键从 env 归还/摘除（有标记才动，无标记一律不碰）。
/// reset 还原官方 与 apply 切到非 deepseek 模型共用同一份归还逻辑（opus P1-4 会审）。
fn restore_window_keys(
    env: &mut serde_json::Map<String, Value>,
    prov: &mut serde_json::Map<String, Value>,
) {
    for k in ["CLAUDE_CODE_AUTO_COMPACT_WINDOW", "CLAUDE_CODE_MAX_CONTEXT_TOKENS"] {
        let Some(prev) = prov.remove(k) else { continue };
        match prev {
            serde_json::Value::Null => {
                if env.get(k).and_then(|v| v.as_str()) == Some(DEEPSEEK_AUTO_COMPACT_WINDOW) {
                    env.remove(k);
                }
            }
            v => {
                env.insert(k.into(), v);
            }
        }
    }
}

/// 窗口双键的**归属追踪**：注入时记录每个键注入前的状态，还原时按标记精确归还/摘除。
/// 文件放 `~/.uking/`（跟随测试沙箱 `UKING_TEST_HOME`），损坏/缺失一律当「无标记」。
fn read_window_provenance() -> serde_json::Map<String, Value> {
    let path = crate::installer::user_home_dir()
        .join(".uking")
        .join("claude-env-provenance.json");
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

fn write_window_provenance(prov: &serde_json::Map<String, Value>) -> Result<(), String> {
    let dir = crate::installer::user_home_dir().join(".uking");
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建 .uking 目录失败: {e}"))?;
    atomic_write(
        &dir.join("claude-env-provenance.json"),
        serde_json::to_string_pretty(prov)
            .unwrap_or_else(|_| "{}".into())
            .as_bytes(),
    )
    .map_err(|e| format!("写 provenance 失败: {e}"))
}

/// 还原 Claude Code：只删我们管理的键，别的不动。
fn reset_claude() -> Result<(), String> {
    let path = claude_settings_path();
    let Ok(s) = std::fs::read_to_string(&path) else {
        return Ok(());
    };
    let Ok(mut root) = serde_json::from_str::<Value>(&s) else {
        return Ok(());
    };
    if let Some(env) = root.get_mut("env").and_then(|e| e.as_object_mut()) {
        // 🔴 归属追踪优先（sol 复审 P1：仅凭值匹配会把「用户恰好自配 200000」误判成
        // 我们注入的）。标记文件 ~/.uking/claude-env-provenance.json 记着**注入前**状态：
        //   有标记且原值=null → 是我们补进去的默认值；且仅当现行值仍是特征值才删
        //     （用户事后手改过就尊重现状）；原值有值 → 我们顶掉过用户配置，归还原值；
        //   无标记 → 不是经我们手写进去的（含用户预先自配、老版本遗留），**一律不碰**。
        // 残留一条无害的 200000 默认好过误删用户配置 —— 这是「宁可维持现状」的方向。
        // 标记消费即清，防止下一次还原重复归还。
        let mut prov = read_window_provenance();
        restore_window_keys(env, &mut prov);
        for k in MANAGED_ENV_KEYS {
            // 窗口双键已由归属追踪处理完（或决定不碰），跳过，绝不走无差别删除。
            if *k == "CLAUDE_CODE_AUTO_COMPACT_WINDOW" || *k == "CLAUDE_CODE_MAX_CONTEXT_TOKENS" {
                continue;
            }
            env.remove(*k);
        }
        // 🔴 写序（sol 复审 2026-08-27 P0 实锤）：先落 settings、成功后才消费标记——
        // settings 侧失败时标记仍在，重试仍能精确回收；标记残留最多一次幂等重放，
        // 可确定性收敛。（上一版反了：先消费标记再写 settings，settings 失败后标记
        // 丢失、重试无法回收窗口键 → 不可收敛。）
        atomic_write(&path, serde_json::to_string_pretty(&root).unwrap().as_bytes())
            .map_err(|e| format!("写 settings.json 失败: {e}"))?;
        write_window_provenance(&prov)?;
    }
    Ok(())
}

/// 从一份 config.toml 原文里把 `[mcp_servers.*]` 段**原样**捞出来（含段头和段内所有行）。
///
/// 为什么不引 toml crate 解析再序列化：① 守体积红线；② 更重要的是**原样保留**——
/// 解析再写回会丢注释、改键序、把用户手写的格式重排一遍。我们的职责是「别弄丢」，
/// 不是「帮他整理」。段的结束判据 = 下一个顶格的 `[`（TOML 里段头必须在行首）。
pub fn extract_mcp_servers(src: &str) -> String {
    let mut out = String::new();
    let mut in_mcp = false;
    for line in src.lines() {
        let t = line.trim_start();
        // 段头：`[mcp_servers.xxx]` 或 `[[mcp_servers.xxx]]`，且必须顶格（缩进的是数组元素/续行）
        if line.starts_with('[') {
            in_mcp = t.starts_with("[mcp_servers.")
                || t.starts_with("[[mcp_servers.")
                || t == "[mcp_servers]";
            if in_mcp {
                out.push_str(line);
                out.push('\n');
            }
            continue;
        }
        if in_mcp {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

fn apply_codex(p: &ProviderPreset, key: &str, model_override: Option<&str>) -> Result<(), String> {
    if key.trim().is_empty() {
        return Err("API Key 不能为空".into());
    }
    if !valid_provider_id(&p.id) {
        return Err("provider id 非法，拒绝写入 Codex 配置".into());
    }
    let dir = codex_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建 .codex 目录失败: {e}"))?;

    let cfg = dir.join("config.toml");
    let old = std::fs::read_to_string(&cfg).unwrap_or_default();
    // 不是我们写的才备份（保住用户原始配置）
    let ours = old.contains("managed by U-King");
    if !ours {
        backup_once(&cfg);
    }
    // 🔴 MCP 连接器必须原样带过来。`codex mcp add` 写的就是本文件的 `[mcp_servers.*]`
    // （已用临时 CODEX_HOME 实测），而下面是**整文件覆盖** —— 不捞出来，客户每切一次驱动
    // 就把自己挂的连接器全抹掉一次，且毫无提示。这跟「不许抢客户模型/登录态」是同一条红线：
    // 我们只该管驱动那几个键，别的都是他的东西。
    let kept_mcp = extract_mcp_servers(&old);

    let model = effective_codex_model(p, model_override);
    // 🔴 一律写 `responses`，**永远不要再写 `chat`**（Issue #364）。
    // 新版 Codex 移除了这个值，而它的失败方式是最坏的一种：不是「这个 provider 不可用」，
    // 是 `Error loading config.toml` → **整份配置拒绝加载 → Codex 秒退**（客户日志里是
    // 每次 `code=1 total=0s events=0`），连别的 provider 和用户自己挂的 MCP 一起废掉。
    // 老的 `chat` 分支是为 0.8x CLI 留的，那个版本早已绝迹，故整条链路删掉不再保留。
    let wire = WIRE_API;
    // 🔴 **`disable_response_storage` 已删（2026-08-24）—— 它是一个死键，而且是会致命的那种死法。**
    //
    // 原意是「中转上游不一定支持服务端会话存储」。但 codex 0.149.0 实测：它和一个我现编的
    // 乱码键 `definitely_bogus_key_xyz` 得到**逐字相同**的对待 ——
    //   `codex --strict-config exec` → `unknown configuration field 'disable_response_storage'`
    // 默认模式静默忽略，所以「看起来一直没出事」；可它的意图**从 0.149 起没有任何配置在承载**，
    // 留着它 = 一份自欺的注释（08-11 的会话记录里就写着「已是未知键未修」，欠了 13 天）。
    //
    // 真正必须删的理由不是「没用」，是**它和 #364 那个 `wire_api = "chat"` 是同一种雷**：
    // 任何走 `--strict-config` 的调用路径会因为这一行**整份 config.toml 拒绝加载**，
    // 连用户自己挂的 MCP 一起废掉。上面那段注释刚讲完这个失败模式，下面一行就在犯它。
    let extra = "";
    // 新版 Codex 对自定义 provider 的 env_key 只认环境变量、不读 auth.json（实测 0.139 报
    // Missing environment variable）。所以 responses 链路把 key 直接写进 provider 块
    // （experimental_bearer_token，Codex++ 同款手法）；chat 链路保持 env_key 兼容老 CLI。
    let auth_line = format!("experimental_bearer_token = \"{}\"", toml_basic_string(key));
    // 新版 Codex 桌面 App 接第三方（中转）Provider 时，调内置 imagegen 技能会「生成完但图不显示」——
    // 根因是 App 对自定义 provider 有权限校验：得 ① 关掉 OpenAI 官方账号鉴权要求（requires_openai_auth）；
    // ② 补上客户端输出图片所需的 actor 授权头（x-openai-actor-authorization，值仅需存在，中转侧透传/忽略）。
    // 虾盘云正是中转 provider，故 responses 链路（新版 CLI + App 共用）统一补齐这两项。
    // 两键均为 Codex 合法字段（codex 0.140 `--strict-config` 实测认可、不报未知）；头值用 ASCII 常量避免非法 HTTP 头。
    // 值最终以真机 Codex App 生图为准（开发机无法验 App 渲染）。
    let app_imagegen_fix =
        "requires_openai_auth = false\nhttp_headers = { \"x-openai-actor-authorization\" = \"u-king\" }\n";
    // ⚠️ 关于客户反馈的「5 次重连」：**查过了，故意不在这里调重试参数**（0.9.84）。
    //
    // 那个 5 是 Codex 自己的 `stream_max_retries` 默认值（上游 model-provider-info：
    // `DEFAULT_STREAM_MAX_RETRIES = 5`，另有 `request_max_retries=4`、
    // `stream_idle_timeout_ms=300_000`）。三个键在 0.145.0 的 provider 块里都合法
    // （已用临时 CODEX_HOME 实测 `config.toml parse ok`），写进去很容易 —— 但不该写：
    //
    //  · `stream_max_retries` 调大 = **重复计费**。上游 `responses_retry.rs` 的重试是
    //    「retry the request loop」，重发整轮请求，不是断点续传；默认 5 次已经意味着最坏
    //    情况把输入 token 烧 5 遍。为了成功率把它调到 10，就是让客户在网络烂的时候烧 10 遍。
    //  · `stream_idle_timeout_ms` 默认已是 5 分钟，而客户的现象是流真的断了 5 次、不是被
    //    误判超时 —— 调它治不了症，只会让失败来得更慢。
    //
    // 真因在中转链路（跨境 + 上游中转），客户端配置改不动它。客户端能做、也已经做了的是
    // **别把重连状态吞掉**（见 agent/codex.rs 的 `notice` 事件）：以前吞了，客户看到的是
    // 「卡住不动」，那才是「5 次重连」被反复投诉的观感来源。
    let toml = format!(
        r#"# managed by U-King —— 驱动切换写入（还原请在 U-King 里选「官方直连」）
# Codex CLI 和 Codex 桌面 App 共用本文件，切一次两边生效
model = "{model}"
model_provider = "{id}"
{extra}
[model_providers.{id}]
name = "{name}"
base_url = "{base}"
{auth_line}
wire_api = "{wire}"
{app_imagegen_fix}"#,
        id = p.id,
        model = toml_basic_string(&model),
        name = toml_basic_string(&p.name),
        base = toml_basic_string(&p.openai_base),
    );
    // 把客户自己挂的 MCP 连接器接回文件末尾（放最后：TOML 段一旦开始就到下一个段头为止，
    // 插在中间会把我们后面的顶层键吃进那个段里）。
    let toml = if kept_mcp.is_empty() {
        toml
    } else {
        format!("{toml}\n# —— 以下为你自己挂的 MCP 连接器，U-King 原样保留 ——\n{kept_mcp}")
    };
    atomic_write(&cfg, toml.as_bytes()).map_err(|e| format!("写 config.toml 失败: {e}"))?;

    // auth.json：
    //  - responses 链路 key 已写进 config.toml 的 experimental_bearer_token，**不碰 auth.json**
    //    （保住用户 Codex 桌面版的 ChatGPT 官方登录凭据，对齐 cc-switch「保护官方登录」）。
    //  - chat 链路老 CLI 仍从 auth.json 读 OPENAI_API_KEY，但改成 **merge 单键**而非整文件覆盖：
    //    只更新 OPENAI_API_KEY，保留用户其它字段（OAuth tokens 等），避免冲掉官方登录。
    if wire != "responses" {
        let auth = dir.join("auth.json");
        backup_once(&auth);
        let mut root: Value = std::fs::read_to_string(&auth)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(|| json!({}));
        if let Some(obj) = root.as_object_mut() {
            obj.insert("OPENAI_API_KEY".into(), json!(key));
        } else {
            root = json!({ "OPENAI_API_KEY": key });
        }
        atomic_write(&auth, serde_json::to_string_pretty(&root).unwrap().as_bytes())
            .map_err(|e| format!("写 auth.json 失败: {e}"))?;
    }
    Ok(())
}

/// 「一键配好全部 AI」该不该动 Codex —— 不该动就返回一句人话理由。
///
/// 规矩：**客户已经有能用的东西时，自动流程不许替他做主**（宪法第 10 条：不碰用户真实状态）。
/// 具体两种情况要让路：
///   ① Codex 桌面版已经用 ChatGPT 官方登录（`auth.json` 里有 tokens）—— 那是他花钱买的会员，
///      我们把 model_provider 改掉，他打开桌面版会发现「登录还在但用的不是官方模型了」，
///      比报错更难查。
///   ② `config.toml` 是他自己写的（没有我们的标记）—— 那是他调好的配置，不是空地。
///
/// 已经被我们接管过（config.toml 带标记）则放行：那是他上次自己点的，再点一次是幂等重配。
/// 注意这个门禁**只管自动批量那条路**；在「Codex 工作站」里显式点切换是明确授权，照切不误。
fn codex_auto_config_blocked() -> Option<String> {
    let dir = codex_dir();

    // 我们接管过 → 放行（幂等重配）
    let cfg = dir.join("config.toml");
    let ours = std::fs::read_to_string(&cfg)
        .map(|s| s.contains("managed by U-King"))
        .unwrap_or(false);
    if ours {
        return None;
    }

    // ① 官方登录态在
    let auth = dir.join("auth.json");
    if let Ok(text) = std::fs::read_to_string(&auth) {
        if let Ok(v) = serde_json::from_str::<Value>(&text) {
            let has_tokens = v
                .get("tokens")
                .map(|t| t.is_object() && t.as_object().map(|o| !o.is_empty()).unwrap_or(false))
                .unwrap_or(false);
            if has_tokens {
                return Some("检测到 ChatGPT 官方登录，已跳过（想省钱可在「Codex 工作站」里手动切）".into());
            }
        }
    }

    // ② 客户自己的配置
    if cfg.exists() {
        return Some("检测到你自己的 config.toml，已跳过（想省钱可在「Codex 工作站」里手动切）".into());
    }

    None
}

/// 还原 Codex 官方直连。**首要目标是保住官方登录态**，其次才是清干净我们写的东西。
///
/// ## 曾经的做法会把客户的 ChatGPT 登录删掉
/// 老代码判断「这文件是不是我们写的」用的是
/// `s.contains("managed by U-King") || s.contains("OPENAI_API_KEY")`。
/// 而 Codex 桌面版走 ChatGPT 官方登录时，`auth.json` 长这样：
/// ```json
/// { "OPENAI_API_KEY": null, "tokens": { "access_token": "…", "refresh_token": "…" } }
/// ```
/// —— 它**天然含有 `OPENAI_API_KEY` 这个字符串**。于是：
///   客户官方登录 → 点「一键配虾盘云」（responses 链路，我们压根没碰 auth.json，也就没备份）
///   → 再点「官方直连（还原）」→ 没备份 + 命中关键字 → **auth.json 被删** → 官方登录没了，
///   得重新扫码登录。我们从没写过那个文件，却把它删了。
///
/// ## 现在的规矩
/// - `config.toml`：只认**我们自己的标记**。没标记 = 客户自己的配置，一个字都不动
///   （老的 `OPENAI_API_KEY` 关键字会误伤客户手写的 `env_key = "OPENAI_API_KEY"`）。
/// - `auth.json`：**永远不删**。有备份就回滚；没备份说明我们没接管过它，原样留着。
///   chat 链路当初是 merge 进去一个键的，还原就把那一个键 merge 出来 —— 进出对称，
///   不碰 tokens 等任何其它字段。
fn reset_codex() -> Result<(), String> {
    let dir = codex_dir();

    // ① config.toml：有备份回滚，否则只删我们自己写的那份
    let cfg = dir.join("config.toml");
    let cfg_bak = dir.join("config.toml.uking-bak");
    if cfg_bak.exists() {
        std::fs::copy(&cfg_bak, &cfg).map_err(|e| format!("还原 config.toml 失败: {e}"))?;
    } else if cfg.exists() {
        let ours = std::fs::read_to_string(&cfg)
            .map(|s| s.contains("managed by U-King"))
            .unwrap_or(false);
        if ours {
            let _ = std::fs::remove_file(&cfg);
        }
    }

    // ② auth.json：绝不删文件。这是官方登录态所在。
    let auth = dir.join("auth.json");
    let auth_bak = dir.join("auth.json.uking-bak");
    if auth_bak.exists() {
        std::fs::copy(&auth_bak, &auth).map_err(|e| format!("还原 auth.json 失败: {e}"))?;
    } else if auth.exists() {
        // 没备份 = 我们没整体接管过。只把 chat 链路可能 merge 进去的那个键摘掉，
        // 且**只在它确实是一把 key 时**才摘（官方登录时这里是 null，摘了反而破坏结构）。
        if let Ok(text) = std::fs::read_to_string(&auth) {
            if let Ok(mut root) = serde_json::from_str::<Value>(&text) {
                let had_key = root
                    .get("OPENAI_API_KEY")
                    .and_then(|v| v.as_str())
                    .map(|s| !s.is_empty())
                    .unwrap_or(false);
                if had_key {
                    if let Some(obj) = root.as_object_mut() {
                        obj.remove("OPENAI_API_KEY");
                    }
                    let _ = atomic_write(
                        &auth,
                        serde_json::to_string_pretty(&root).unwrap_or_default().as_bytes(),
                    );
                }
            }
        }
    }
    Ok(())
}

/// OpenClaw **CLI** 配置路径：`~/.openclaw/openclaw.json`（支持 UKING_TEST_HOME 沙箱）。
/// 注意：ClawX 4.x **图形版**不读这个文件，用的是 clawx_providers_path()（见 apply_clawx）。
/// 这个保留给隐藏的 OpenClaw CLI 用（write_openclaw_xiapan 走 installer.rs 自己的实现）。
#[allow(dead_code)]
fn openclaw_config_path() -> PathBuf {
    config_home().join(".openclaw").join("openclaw.json")
}

/// Hermes 配置目录 —— 转调 `installer::hermes_config_dir()`（全仓唯一真相源）。
///
/// 这里**故意不再自带一份实现**：这个函数曾经和 `installer.rs` 里的孪生兄弟各写一遍，
/// 两份一起押在「Windows 上是 `%LOCALAPPDATA%\hermes`」这个错误假设上，于是一起错。
/// 同一事实存在几份就会漂移几份（宪法第 8 条）—— 判据只留一处，改的时候没有第二个地方能忘。
fn hermes_dir() -> PathBuf {
    crate::installer::hermes_config_dir()
}

/// 从一份 Hermes `config.yaml` 里读出 `model:` 块的某个子键（朴素行解析，够用）。
/// 只在 `model:` 顶层块内匹配，避免撞上别的段里的同名键。
fn read_hermes_model_key(text: &str, key: &str) -> Option<String> {
    let mut in_model = false;
    for line in text.lines() {
        let line = strip_bom(line);
        let is_top = !line.starts_with(char::is_whitespace) && !line.trim().is_empty();
        if is_top {
            in_model = line.trim_start().starts_with("model:");
            continue;
        }
        if !in_model {
            continue;
        }
        let child = line.trim_start();
        if let Some(rest) = child.strip_prefix(key) {
            if let Some(v) = rest.strip_prefix(':') {
                let v = v.trim().trim_matches('"').trim_matches('\'');
                if !v.is_empty() {
                    return Some(v.to_string());
                }
            }
        }
    }
    None
}

/// 剥掉 UTF-8 BOM。PowerShell `Set-Content -Encoding UTF8` / 记事本另存都会带 BOM，
/// 而 U+FEFF 不是 whitespace，`trim_start` 剥不掉它 —— 不剥的话 `\ufeffmodel:` 匹配不上
/// `"model:"`，`set_yaml_model_block` 会把整个 model 块当成普通行保留、再追加第二个
/// model 块（YAML 重复顶层键，Hermes 读配置行为未定义）；`read_hermes_model_key`
/// 也读不到 `api_mode`（端点错配 404 的另一条路）。
fn strip_bom(s: &str) -> &str {
    s.strip_prefix('\u{feff}').unwrap_or(s)
}

/// ★ **把写错地方的 Hermes 配置搬回来**（pc-*** 一类客户机的自愈路径）。
///
/// 0.9.90 及以前，Windows 上 U-King 把虾盘云端点+Key 写进了 `%LOCALAPPDATA%\hermes`
/// —— 那是 Hermes 的安装目录，它运行时只读 `HERMES_HOME` / `~/.hermes`。结果客户机上
/// 两份配置各说各话：模型名是我们写的（`deepseek-v4-flash`），端点还是别人的
/// （`api.deepseek.com/v1`）→ 拿虾盘云的模型名打 DeepSeek 官方 → **404**。
///
/// 光把落点改对**不够**：客户升级后如果不再点一次「一键配好全部 AI」，真 home 里那份坏配置
/// 原封不动，照样 404。所以启动时补这一刀。
///
/// 三条自律，别放宽：
/// ① **只认自己的东西**——旧落点的 `config.yaml` 必须带 U-King 标记（`active_profile:
///    U-King 虾盘云`）才搬。别人的配置一个字节都不碰。
/// ② **不覆盖已经对的**——真 home 的 `base_url` 已经等于旧落点的，直接跳过（幂等，
///    每次启动跑也不会反复写盘）。
/// ③ **留底**——沿用 `backup_once`，跟 `apply_hermes` 同一套备份约定，可回滚。
///
/// 旧落点**不删**（宪法第 10 条：不静默删用户机器上我们没把握的东西），只是从此不再写它。
/// 返回 `Some(说明)` 表示真搬了一次；`None` = 无需迁移。
pub fn migrate_hermes_config_from_legacy() -> Option<String> {
    let legacy = crate::installer::hermes_legacy_dir()?;
    let live = hermes_dir();
    // HERMES_HOME 正好指着旧落点（U-Hermes U 盘版等）→ 那它就是真 home，没什么可搬的。
    if legacy == live {
        return None;
    }
    let legacy_cfg_text = std::fs::read_to_string(legacy.join("config.yaml")).ok()?;
    // ① 只搬我们自己写的
    if !legacy_cfg_text.contains("U-King") {
        return None;
    }
    let base = read_hermes_model_key(&legacy_cfg_text, "base_url")?;
    let model = read_hermes_model_key(&legacy_cfg_text, "default")?;
    let provider = read_hermes_model_key(&legacy_cfg_text, "provider").unwrap_or_else(|| "custom".into());
    // 搬家时顺手把 api_mode 归一 —— 旧落点很可能压根没这个键（U-King 2026-08-19 之前不写），
    // 照搬过来仍然是「交给 Hermes 按主机名猜」。只保留 anthropic 那一档，其余一律 chat_completions。
    let legacy_api_mode = read_hermes_model_key(&legacy_cfg_text, "api_mode").unwrap_or_default();
    let profile_name = read_hermes_model_key(&legacy_cfg_text, "active_profile")
        .unwrap_or_else(|| "U-King 托管".into());
    let api_mode_value = if legacy_api_mode.contains("anthropic") {
        HERMES_API_MODE_ANTHROPIC
    } else {
        HERMES_API_MODE_CHAT
    };
    // Key 优先取旧落点 .env 里的（那才是 Hermes 真正认的凭据位），config.yaml 兜底。
    let legacy_env = std::fs::read_to_string(legacy.join(".env")).unwrap_or_default();
    let key = read_env_var(&legacy_env, "OPENAI_API_KEY")
        .or_else(|| read_hermes_model_key(&legacy_cfg_text, "api_key"))?;

    let live_cfg = live.join("config.yaml");
    let live_cfg_text = std::fs::read_to_string(&live_cfg).unwrap_or_default();
    let live_env_file = live.join(".env");
    let live_env_text = std::fs::read_to_string(&live_env_file).unwrap_or_default();
    // ② 两处都已经对了 → 幂等跳过
    if read_hermes_model_key(&live_cfg_text, "base_url").as_deref() == Some(base.as_str())
        && read_env_var(&live_env_text, "OPENAI_BASE_URL").as_deref() == Some(base.as_str())
    {
        return None;
    }

    if std::fs::create_dir_all(&live).is_err() {
        return None;
    }
    // ③ 留底后写。用的就是 apply_hermes 那两个写入器，不另起一套。
    backup_once(&live_cfg);
    let new_cfg = set_yaml_model_block(
        &live_cfg_text,
        &model,
        &provider,
        &profile_name,
        &base,
        &key,
        api_mode_value,
    );
    atomic_write(&live_cfg, new_cfg.as_bytes()).ok()?;

    backup_once(&live_env_file);
    let mut env_text = set_env_var(&live_env_text, "OPENAI_API_KEY", &key);
    env_text = set_env_var(&env_text, "OPENAI_BASE_URL", &base);
    atomic_write(&live_env_file, env_text.as_bytes()).ok()?;

    Some(format!(
        "已把 Hermes 配置从旧落点 {} 迁到 {}（端点 {base}，模型 {model}）",
        legacy.display(),
        live.display()
    ))
}

/// ★ **Hermes 落点诊断**（无头入口 `--hermes-where`）。
///
/// 为什么值得单开一条：pc-*** 报的是 `HTTP 404`，日志里写着 `Provider: custom`、
/// `Endpoint: https://api.deepseek.com/v1`，而我们的界面一路显示「配置成功」——
/// 因为文件确实写成功了，只是写在了 Hermes 不读的目录。这类「报告是对的、世界是坏的」
/// 的故障，**看我们自己的状态一辈子也看不出来**，只能把两个落点摆在一起对。
///
/// 输出是**非隐私口径**（只出路径和端点，Key 一律不出），可以直接让客户贴过来。
pub fn hermes_where() -> serde_json::Value {
    let live = hermes_dir();
    let read = |dir: &PathBuf| -> (Option<String>, Option<String>, Option<String>) {
        let cfg = std::fs::read_to_string(dir.join("config.yaml")).unwrap_or_default();
        let env = std::fs::read_to_string(dir.join(".env")).unwrap_or_default();
        (
            read_hermes_model_key(&cfg, "base_url"),
            read_hermes_model_key(&cfg, "default"),
            read_env_var(&env, "OPENAI_BASE_URL"),
        )
    };
    let (live_base, live_model, live_env_base) = read(&live);
    let legacy = crate::installer::hermes_legacy_dir();
    let legacy_info = legacy.as_ref().map(|d| {
        let (b, m, e) = read(d);
        serde_json::json!({
            "dir": d.display().to_string(),
            "config_base_url": b, "config_model": m, "env_base_url": e,
        })
    });

    // 客户机上那个 404 的**特征签名**：模型名是我们发的（虾盘云的 deepseek-v4-* / gpt-*），
    // 端点却不是我们的 —— 拿虾盘云的模型名去打别人家，必 404。判「像不像我们的模型名」
    // 用的是「配置里出现过我们的端点」这个更硬的旁证，避免把用户自己配的 DeepSeek 官方
    // 账号误判成故障（那是人家的自由，不是 bug）。
    let ours = |u: &Option<String>| u.as_deref().is_some_and(is_xiapan_endpoint);
    let effective = live_env_base.clone().or_else(|| live_base.clone());
    let managed_by_us = ours(&live_base)
        || legacy_info
            .as_ref()
            .is_some_and(|l| l["config_base_url"].as_str().is_some_and(is_xiapan_endpoint));
    let mismatch = managed_by_us && !ours(&effective);

    serde_json::json!({
        // Hermes 自己算出来的家目录（hermes_constants.get_hermes_home）：HERMES_HOME → ~/.hermes
        "live_dir": live.display().to_string(),
        "hermes_home_env": std::env::var("HERMES_HOME").ok(),
        "config_base_url": live_base,
        "config_model": live_model,
        // 🔴 provider=custom 时 Hermes 的端点真正来自 .env 的 OPENAI_BASE_URL（实测 source=env/config），
        // 所以这一行比 config.yaml 那行更能决定「实际打到哪」。
        "env_base_url": live_env_base,
        "effective_base_url": effective,
        "legacy": legacy_info,
        "mismatch": mismatch,
        "verdict": if mismatch {
            "错配：模型名是虾盘云的、端点不是 —— 拿我们的模型名打别人家，必 404。跑一次「一键配好全部 AI」或重启 U-King（启动会自动迁移一次）。"
        } else if live_base.is_none() && live_env_base.is_none() {
            "这台机器上 Hermes 还没被配过（或没装）。"
        } else {
            "落点一致，没有本次这类错配。"
        },
    })
}

/// dotenv 风格读取：取 `KEY=value` 的值（忽略注释行）。与 `set_env_var` 对称。
fn read_env_var(text: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}=");
    // 后出现的覆盖先出现的 —— 与 python-dotenv 的 last-wins 一致。
    let mut found = None;
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with('#') {
            continue;
        }
        if let Some(v) = l.strip_prefix(&prefix) {
            let v = v.trim().trim_matches('"').trim_matches('\'');
            if !v.is_empty() {
                found = Some(v.to_string());
            }
        }
    }
    found
}

/// cc-switch 式接管 Hermes 模型：
/// - `~/.hermes/config.yaml` 设 `model.default` + `model.provider`
/// - `~/.hermes/auth.json` 的 `credential_pool` 注入一条凭据（虾盘云 OpenAI 兼容端点 + key）
///
/// Hermes 凭据是 OpenAI 兼容格式（base_url + access_token）。虾盘云 `/v1` 直接喂，
/// 修「key 空 → 403 Request not allowed」。只动 model + 我们这条凭据，其它不碰。
fn apply_hermes(p: &ProviderPreset, key: &str, model: &str) -> Result<(), String> {
    if key.trim().is_empty() {
        return Err("API Key 不能为空".into());
    }
    let dir = hermes_dir();
    // ⚠️ 端点必须跟 Hermes 的 api_mode 匹配（pc-*** 客户机实锤的 404 根因）：
    // 老配置 `api_mode: anthropic_messages` + OpenAI 端点 → Hermes 请求 {base}/v1/messages
    // → DeepSeek 官方 404（/v1 只有 /chat/completions），重试 3 次全失败 → 工具完全不可用。
    // 所以读现有 api_mode 选端点：anthropic → anthropic_base（DeepSeek 官方
    // api.deepseek.com/anthropic / 虾盘云 api.u-claw.org 均提供 Anthropic 兼容路由）；
    // 其它/无 → openai_base（现状语义，openai_chat 模式）。
    let existing_cfg_text = std::fs::read_to_string(dir.join("config.yaml")).unwrap_or_default();
    let api_mode = read_hermes_model_key(&existing_cfg_text, "api_mode").unwrap_or_default();
    // 端点和 api_mode 必须成对决定 —— 只定端点、把 api_mode 留给 Hermes 猜，就是本函数
    // 头部注释里那个 `/v1/responses` 500 的成因。下面这个元组是「唯一一处」定这两件事的地方。
    let (base, api_mode_value) = if api_mode.contains("anthropic") {
        let b = p.anthropic_base.clone().ok_or_else(|| {
            format!(
                "{} 不支持 Anthropic 模式（Hermes 当前 api_mode={api_mode}，换个支持 Anthropic 兼容端点的供应商，或先把 Hermes 的 api_mode 改成 {HERMES_API_MODE_CHAT}）",
                p.name
            )
        })?;
        (b, HERMES_API_MODE_ANTHROPIC)
    } else {
        // 其余一律钉死 chat_completions：虾盘云这类 OpenAI 兼容中转只有 /chat/completions。
        // 包含 api_mode 是 codex_responses / responses / 空 / 乱写 的情况 —— **都要被纠回来**，
        // 因为 apply 的语义是「把这台机器配成能用」，不是「尊重一个会 500 的旧值」。
        (p.openai_base.clone(), HERMES_API_MODE_CHAT)
    };
    if base.trim().is_empty() {
        return Err(format!("{} 不支持 Hermes（缺 OpenAI 兼容端点）", p.name));
    }
    if !dir.exists() {
        // Claude Code 稳的一个关键点是“配置目录不存在也能写进去”。Hermes 的 pip 包
        // `hermes --version` 不一定会创建配置目录，所以这里主动建，做到装完即可预配置。
        std::fs::create_dir_all(&dir).map_err(|e| format!("创建 Hermes 配置目录失败: {e}"))?;
    }
    // provider 名：**统一用 "custom"** —— Hermes 各版本都认的「通用 OpenAI 兼容」provider，
    // 凭据走 .env 的 OPENAI_API_KEY / OPENAI_BASE_URL。**不再用内置名（deepseek/glm/kimi）**：
    // 某些版本（v0.17.0 实测，见 memory hermes-provider-name-must-be-builtin）把 deepseek 从
    // 内置 provider 表删了 → `Unknown provider` 直接挂；而 "custom" 永远在表里，虾盘云 /
    // DeepSeek / GLM / Kimi 这些 OpenAI 兼容端点都对得上 Authorization: Bearer。
    // 真机实测：pc-*** Hermes v0.16.0，provider:custom + 仅 OPENAI_*（.org.cn）→ HELLO_OK。
    let provider_name = "custom";

    // 1) config.yaml：设整个 model 块（active_profile/provider/base_url/default/auth_* + api_key）。
    //    作用是定**端点**（base_url）+ **模型**（default）+ **provider**（custom）。
    //    ⚠️ **凭据真正从 .env 读**（见 1.5）—— config.yaml.model.api_key Hermes 基本不认：
    //    pc-*** 实锤，config.yaml 写了 api_key 但 .env 没 key 时，Hermes 仍报 `no API key found`。
    //    所以 1.5 的 .env 那步才是命门。
    let cfg = dir.join("config.yaml");
    backup_once(&cfg);
    let mut text = existing_cfg_text.clone();
    text = set_yaml_model_block(
        &text,
        model,
        provider_name,
        &format!("U-King {}", p.name),
        &base,
        key,
        api_mode_value,
    );
    atomic_write(&cfg, text.as_bytes()).map_err(|e| format!("写 hermes config.yaml 失败: {e}"))?;

    // 1.5) .env：**真正的凭据路径**，命门所在。provider=custom 时 Hermes 就吃 `.env` 的
    //   OPENAI_API_KEY / OPENAI_BASE_URL（真机实测仅这两个键即可 → HELLO_OK，无需 DEEPSEEK_* 等）。
    //   ⚠️ 必须**无条件写**（含 .env 不存在时创建）—— 老代码 `if env_file.exists()` 把没有 .env 的
    //   干净机整段跳过 → key 一次都没落地 → Hermes 报 `no API key found`（pc-*** 实锤 2026-06-24，
    //   见 memory uking-hermes-env-key-not-written-bug）。Claude Code 那条路（settings.json env 块）
    //   无条件写，所以从不坏；Hermes 这条路被这个 exists 闸坑了，是「Hermes 一直就不是好的」的真因。
    //   atomic_write 写裸字节（无 BOM，python-dotenv 不会把首行键名连 BOM 吞掉）；只改这两个键、保留其它行。
    let env_file = dir.join(".env");
    backup_once(&env_file); // 不存在 → no-op；存在 → 留底
    let mut env_text = std::fs::read_to_string(&env_file).unwrap_or_default();
    env_text = set_env_var(&env_text, "OPENAI_API_KEY", key);
    env_text = set_env_var(&env_text, "OPENAI_BASE_URL", &base);
    atomic_write(&env_file, env_text.as_bytes()).map_err(|e| format!("写 hermes .env 失败: {e}"))?;

    // 2) auth.json：best-effort 也往 credential_pool[provider] 写一条（部分 Hermes 版本会读），
    //    但**不是主路径** —— 主路径是 .env 的 OPENAI_*（见 1.5）。写失败不致命。
    let auth = dir.join("auth.json");
    backup_once(&auth);
    let mut root: Value = std::fs::read_to_string(&auth)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({ "version": 1, "providers": {}, "credential_pool": {} }));
    if !root.is_object() {
        root = json!({ "version": 1, "providers": {}, "credential_pool": {} });
    }
    let obj = root.as_object_mut().unwrap();
    let pool = obj
        .entry("credential_pool")
        .or_insert_with(|| json!({}));
    if let Some(pool) = pool.as_object_mut() {
        pool.insert(
            provider_name.to_string(),
            json!([{
                "id": "uking",
                "label": "UKING_KEY",
                "auth_type": "api_key",
                "priority": 0,
                "source": "uking",
                "access_token": key,
                "base_url": base,
                "request_count": 0
            }]),
        );
    }
    let _ = atomic_write(&auth, serde_json::to_string_pretty(&root).unwrap().as_bytes());
    Ok(())
}

/// 整块替换 YAML 里的 `model:` 块（朴素行处理，避免引 yaml crate）。
/// 写全 Hermes 实际需要的字段：active_profile / provider / base_url / default /
/// auth_mode / auth_header / auth_scheme（实测真实 config 的 model 段就这些）。
/// 保留 model 块里我们不管的子键（temperature/max_tokens 等）原样；其它顶层段不动。
/// 没有 model 块就追加一个。
/// Hermes 的 `model.api_mode` 取值（**不是** transport 名）。
///
/// 🔴 **两套词汇别混**：`openai_chat` / `anthropic_messages` / `codex_responses` 是 Hermes 内部的
/// **transport** 名；写进 config.yaml 的 `api_mode` 是另一套 —— `hermes_cli/providers.py` 里的
/// `TRANSPORT_TO_API_MODE` 负责映射，`openai_chat` → **`chat_completions`**。
/// 本文件原来的报错文案让客户「把 api_mode 改成 openai_chat」，那是个 transport 名；
/// 它能work只是因为 `config.py::_API_MODE_ALIASES` 恰好收了这个别名。写规范值。
const HERMES_API_MODE_CHAT: &str = "chat_completions";
const HERMES_API_MODE_ANTHROPIC: &str = "anthropic_messages";

/// 写 Hermes 的 model 块。
///
/// 🔴 **`api_mode` 必须显式写死，不能留空让 Hermes 自己猜。**（2026-08-19 定，客户实锤）
/// Hermes 在 `api_mode` 缺失或不认识时会**按主机名猜 transport**，而这个猜测可能落到
/// `codex_responses` —— 那会让请求打 `POST {base}/v1/responses`。虾盘云中转只实现了
/// `/chat/completions`，于是网关回
/// `500 {"code":"convert_request_failed","message":"not implemented"}`，重试 3 次耗尽退出。
/// 上游自己记着这个坑（`hermes_cli/config.py::_API_MODE_ALIASES` 的注释，#66543，
/// 线上对 api.actual.inc 实测）：「在这张别名表之前，无法识别的 api_mode 会被静默忽略、
/// transport 退回按主机名猜，于是 `api_mode: openai` 的配置在升级后翻成 codex_responses
/// 把 provider 打挂」。
///
/// U-King 原来只写 `provider: custom` + `base_url`，**从不写 api_mode** —— 全新安装出来的
/// config.yaml 里根本没这个键，等于把 transport 的决定权交给了那套猜测。
/// ★ 这是「配置成功了、机器还是坏的」那一类：apply 全绿、报告说配好了，客户一跑就 500。
fn set_yaml_model_block(
    text: &str,
    model: &str,
    provider: &str,
    profile_name: &str,
    base_url: &str,
    api_key: &str,
    api_mode: &str,
) -> String {
    // U-King 管理的 model 子键 → 目标值。其余子键（temperature 等）保留原行。
    // ⚠️ api_key 必须在块内 —— Hermes 实际从 model.api_key 取凭据（实测能连通的备份就这样）。
    let managed: &[(&str, String)] = &[
        ("active_profile", profile_name.to_string()),
        ("provider", provider.to_string()),
        ("base_url", base_url.to_string()),
        ("api_mode", api_mode.to_string()),
        ("default", model.to_string()),
        ("api_key", api_key.to_string()),
        ("auth_mode", "api_key".to_string()),
        ("auth_header", "Authorization".to_string()),
        ("auth_scheme", "Bearer".to_string()),
    ];
    let is_top_key = |l: &str| !l.starts_with(char::is_whitespace) && !l.trim().is_empty();

    let lines: Vec<String> = text.lines().map(|s| s.to_string()).collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len() + 8);
    let mut i = 0;
    let mut saw_model = false;

    while i < lines.len() {
        let line = strip_bom(&lines[i]);
        if is_top_key(line) && line.trim_start().starts_with("model:") {
            saw_model = true;
            out.push("model:".into());
            // 收集原 model 块内的子键行（到下一个顶层键为止），保留我们不管的
            let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
            for (k, v) in managed {
                out.push(format!("  {k}: {v}"));
                seen.insert(k);
            }
            i += 1;
            while i < lines.len() && !is_top_key(strip_bom(&lines[i])) {
                let child = lines[i].trim_start();
                let key = child.split(':').next().unwrap_or("");
                // 我们管的键已写过 → 跳过原行；不管的（temperature 等）原样保留
                if !managed.iter().any(|(k, _)| *k == key) {
                    out.push(lines[i].clone());
                }
                i += 1;
            }
            continue;
        }
        out.push(line.to_string());
        i += 1;
    }

    if !saw_model {
        out.push("model:".into());
        for (k, v) in managed {
            out.push(format!("  {k}: {v}"));
        }
    }

    let mut s = out.join("\n");
    if !s.ends_with('\n') {
        s.push('\n');
    }
    s
}

/// dotenv 风格 upsert：把 `KEY=value` 设成目标值，保留文件其它行。
/// - 已有未注释的 `KEY=...` → 原地替换值（保留其在文件中的位置）。
/// - 只有注释掉的 `# KEY=...` → 在该注释行后插入启用的真实行（保留注释作参考）。
/// - 完全没有 → 追加到文件末尾。
/// 不引 dotenv crate，朴素行处理（值不含换行，够用）。
fn set_env_var(text: &str, key: &str, value: &str) -> String {
    let target = format!("{key}={value}");
    let active_prefix = format!("{key}=");
    let mut out: Vec<String> = Vec::new();
    let mut replaced = false;
    let mut commented_idx: Option<usize> = None;

    for line in text.lines() {
        // BOM 剥掉再判（PowerShell 写的 .env 带 BOM 头时，旧行也要能命中替换而不是重复追加）
        let trimmed = strip_bom(line).trim_start();
        if !replaced && trimmed.starts_with(&active_prefix) {
            // 命中未注释的活动行 → 就地替换
            out.push(target.clone());
            replaced = true;
            continue;
        }
        // 记录第一处被注释掉的同名键（# KEY= 或 #KEY=），用于"启用注释行"分支
        if commented_idx.is_none() {
            let t = trimmed.trim_start_matches('#').trim_start();
            if t.starts_with(&active_prefix) {
                commented_idx = Some(out.len());
            }
        }
        out.push(line.to_string());
    }

    if !replaced {
        match commented_idx {
            // 在注释行之后插入启用的真实行（保留注释作参考）
            Some(idx) => out.insert(idx + 1, target),
            // 整个文件都没有 → 追加到末尾
            None => out.push(target),
        }
    }

    let mut s = out.join("\n");
    if !s.ends_with('\n') {
        s.push('\n');
    }
    s
}

/// 还原 Hermes：优先回滚 config.yaml / auth.json / .env 备份；无备份则删我们写的凭据。
fn reset_hermes() -> Result<(), String> {
    let dir = hermes_dir();
    // .env 也回滚 —— 它才是真正的凭据文件（与 apply_hermes 对称）。
    for name in ["config.yaml", "auth.json", ".env"] {
        let f = dir.join(name);
        let bak = f.with_extension(format!(
            "{}.uking-bak",
            f.extension().and_then(|e| e.to_str()).unwrap_or("cfg")
        ));
        if bak.exists() {
            let _ = std::fs::copy(&bak, &f);
        }
    }
    // 没备份的话，至少把 auth.json 里我们注入的 uking 凭据删掉
    let auth = dir.join("auth.json");
    if let Ok(s) = std::fs::read_to_string(&auth) {
        if let Ok(mut root) = serde_json::from_str::<Value>(&s) {
            if let Some(pool) = root.get_mut("credential_pool").and_then(|p| p.as_object_mut()) {
                pool.remove("uking");
            }
            let _ = atomic_write(&auth, serde_json::to_string_pretty(&root).unwrap().as_bytes());
        }
    }
    Ok(())
}

/// POSIX 上给凭据文件用的 0600 原子写。权限在 temp 创建时就定好，
/// 不能「先 rename 再 chmod」：DSH 的 watcher 可能在两步之间读到 0644 并拒绝加载。
#[cfg(unix)]
fn atomic_write_owner_only(path: &PathBuf, data: &[u8]) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建目录失败: {e}"))?;
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let fname = path.file_name().and_then(|f| f.to_str()).unwrap_or("credential");
    let tmp = path.with_file_name(format!(".{fname}.uking-tmp.{}.{}", std::process::id(), stamp));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)
        .map_err(|e| format!("创建凭据临时文件失败: {e}"))?;
    if let Err(e) = file.write_all(data).and_then(|_| file.sync_all()) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("写凭据临时文件失败: {e}"));
    }
    drop(file);
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("替换凭据文件失败: {e}"));
    }
    match std::fs::read(path) {
        Ok(back) if back == data => Ok(()),
        Ok(back) => Err(format!(
            "凭据写完后回读对不上（写入 {} 字节、读回 {} 字节）: {}",
            data.len(),
            back.len(),
            path.display()
        )),
        Err(e) => Err(format!("凭据写完后读不回来: {}: {e}", path.display())),
    }
}

#[cfg(not(unix))]
fn atomic_write_owner_only(path: &PathBuf, data: &[u8]) -> Result<(), String> {
    atomic_write(path, data)
}

// ============================================================
// DeepSeek Harness（Web / terminal 共用一份模型配置）
// ============================================================

/// settings 只存引用，真 Key 单独存在 DSH 的受管凭据文档里。
const DSH_UKING_CREDENTIAL: &str = "UKING_DSH_API_KEY";

#[derive(Debug, Serialize, Deserialize)]
struct DshDriverBackup {
    /// 接管前 `agent-default-model` 整段的 YAML。None = 当时没有用户层选择。
    previous_default_yaml: Option<String>,
}

fn dsh_driver_backup_path() -> PathBuf {
    config_home().join(".uking").join("dsh-driver-backup.json")
}

fn save_dsh_driver_backup(default: Option<&YamlValue>) -> Result<(), String> {
    let previous_default_yaml = default
        .map(serde_yaml::to_string)
        .transpose()
        .map_err(|e| format!("备份 DSH 原默认模型失败: {e}"))?;
    let payload = serde_json::to_vec_pretty(&DshDriverBackup { previous_default_yaml })
        .map_err(|e| format!("生成 DSH 回滚信物失败: {e}"))?;
    atomic_write(&dsh_driver_backup_path(), &payload).map_err(|e| format!("写 DSH 回滚信物失败: {e}"))
}

/// 读回滚信物。**损坏 = 视作没有备份并就地清掉**，不是硬错。
///
/// 🔴 原来损坏返回 `Err`，而接管（`apply_dsh`）和还原（`reset_dsh`）两条调用方都 `?` 往上抛，
/// **全代码里唯一删除这个文件的地方（`reset_dsh` 末尾）在那个 `?` 的下游** —— 一旦信物损坏：
/// 接管失败、还原也失败、清除损坏文件的代码永远走不到，客户只能自己去手删
/// `~/.uking/dsh-driver-backup.json`。**一个把自己锁死、且唯一出路是手删文件的功能，
/// 对客户等于坏了。**
///
/// 触发门槛还很低：`fs::read` 对 0 字节文件返回成功，`from_slice(&[])` 必然 EOF ——
/// 任何一次 `atomic_write` 被打断留下的空文件就够了（本机 2026-08-18 一天 5 次异常退出）。
///
/// 降级的代价是**丢一个回滚点**：DSH 的默认模型回不到接管前那个选择，改为移除我们写的默认，
/// 客户在 DSH Web 里重选一次即可。比整条路锁死轻得多，而且下一次接管会重新存一份。
fn load_dsh_driver_backup() -> Result<Option<DshDriverBackup>, String> {
    let path = dsh_driver_backup_path();
    match std::fs::read(&path) {
        Ok(bytes) => match serde_json::from_slice(&bytes) {
            Ok(v) => Ok(Some(v)),
            Err(e) => {
                crate::ulog::write(
                    "providers",
                    &format!("DSH 回滚信物损坏（{} 字节），已视作没有备份并删除: {e}", bytes.len()),
                );
                // 就地清掉：留着它会让每次接管/还原都再撞一次同一颗雷。
                let _ = std::fs::remove_file(&path);
                Ok(None)
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("读 DSH 回滚信物失败: {e}")),
    }
}

fn yaml_key(key: &str) -> YamlValue {
    YamlValue::String(key.to_string())
}

/// 读一份 DSH YAML 文档，顶层必须是 mapping。报错只带位置，不回显源行：
/// `.credentials.yaml` 的源行就是密钥，把 parser 原始消息直接打出去会泄密。
fn read_yaml_mapping(path: &PathBuf, label: &str) -> Result<(Option<Vec<u8>>, YamlMapping), String> {
    let bytes = match std::fs::read(path) {
        Ok(v) => Some(v),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("读取 {label} 失败: {e}")),
    };
    let Some(raw) = bytes.as_ref() else {
        return Ok((None, YamlMapping::new()));
    };
    let text = std::str::from_utf8(raw).map_err(|_| format!("{label} 不是 UTF-8，已拒绝覆盖"))?;
    if text.trim().is_empty() {
        return Ok((bytes, YamlMapping::new()));
    }
    let value = serde_yaml::from_str::<YamlValue>(text).map_err(|e| {
        let at = e
            .location()
            .map(|p| format!("（第 {} 行第 {} 列）", p.line(), p.column()))
            .unwrap_or_default();
        format!("{label} 不是合法 YAML{at}，已拒绝覆盖")
    })?;
    match value {
        YamlValue::Mapping(m) => Ok((bytes, m)),
        YamlValue::Null => Ok((bytes, YamlMapping::new())),
        _ => Err(format!("{label} 顶层必须是 mapping，已拒绝覆盖")),
    }
}

fn yaml_mapping_field_mut<'a>(
    parent: &'a mut YamlMapping,
    key: &str,
    label: &str,
) -> Result<&'a mut YamlMapping, String> {
    let k = yaml_key(key);
    if !parent.contains_key(&k) {
        parent.insert(k.clone(), YamlValue::Mapping(YamlMapping::new()));
    }
    match parent.get_mut(&k) {
        Some(YamlValue::Mapping(m)) => Ok(m),
        _ => Err(format!("{label}.{key} 应该是 mapping，已拒绝覆盖")),
    }
}

/// 乐观写：从 parse 到落盘之间若 DSH Web / 另一个 U-King 实例改了文件，
/// 明确拒绝而不是拿旧快照覆盖新事实。
fn write_yaml_mapping(
    path: &PathBuf,
    before: &Option<Vec<u8>>,
    root: &YamlMapping,
    label: &str,
    owner_only: bool,
) -> Result<(), String> {
    let live = match std::fs::read(path) {
        Ok(v) => Some(v),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("重读 {label} 失败: {e}")),
    };
    if &live != before {
        return Err(format!("{label} 刚被另一个程序修改，本次未覆盖；请重试"));
    }
    let text = serde_yaml::to_string(&YamlValue::Mapping(root.clone()))
        .map_err(|e| format!("生成 {label} 失败: {e}"))?;
    let write = if owner_only { atomic_write_owner_only } else { atomic_write };
    write(path, text.as_bytes()).map_err(|e| format!("写入 {label} 失败: {e}"))?;
    Ok(())
}

/// 密钥文件在 POSIX 上必须是 0600，否则 DSH 自己会拒绝读它。
#[cfg(unix)]
fn protect_dsh_credentials(path: &PathBuf) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| format!("设置 DSH 凭据文件权限失败: {e}"))
}

#[cfg(not(unix))]
fn protect_dsh_credentials(_path: &PathBuf) -> Result<(), String> {
    Ok(())
}

/// 把任意拥有 OpenAI-compatible 端点的 U-King provider 写进 DSH。
/// Web 和 terminal profile 都从同一个 `$DSH_HOME` 读 settings/credentials，所以只实现一次。
fn apply_dsh(p: &ProviderPreset, key: &str, model: &str) -> Result<(), String> {
    if key.trim().is_empty() {
        return Err("API Key 不能为空".into());
    }
    let base = p.openai_base.trim();
    if !(base.starts_with("http://") || base.starts_with("https://")) {
        return Err(format!("{} 不支持 DSH（缺 OpenAI 兼容端点）", p.name));
    }
    if model.trim().is_empty() {
        return Err("DSH 模型不能为空".into());
    }

    let dir = dsh_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建 DSH 配置目录失败: {e}"))?;
    let settings_path = dir.join("settings.yaml");
    let credentials_path = dir.join(".credentials.yaml");

    // 先把两份文档都验明是可安全 merge 的 YAML，再动任何一份。
    let (settings_before, mut settings) = read_yaml_mapping(&settings_path, "DSH settings.yaml")?;
    let (credentials_before, mut credentials) =
        read_yaml_mapping(&credentials_path, "DSH .credentials.yaml")?;

    credentials.insert(yaml_key(DSH_UKING_CREDENTIAL), YamlValue::String(key.trim().to_string()));

    let llm = yaml_mapping_field_mut(&mut settings, "llm-pi-ai", "DSH settings.yaml")?;
    let providers = yaml_mapping_field_mut(llm, "providers", "DSH settings.yaml.llm-pi-ai")?;
    let route_id = managed_provider_id(p);
    // `uking-` 是我们的保留命名空间：切供应商时只留当前一条，客户自己的 route 原样保留。
    providers.retain(|key, _| {
        key.as_str()
            .map(|id| !is_managed_provider_id(id))
            .unwrap_or(true)
    });
    let profile = serde_yaml::to_value(json!({
        "displayName": p.name,
        "apiKeyEnv": DSH_UKING_CREDENTIAL,
        "api": "openai-completions",
        "baseURL": base,
        "models": [{ "id": model, "name": model }]
    }))
    .map_err(|e| format!("生成 DSH provider 配置失败: {e}"))?;
    providers.insert(yaml_key(&route_id), profile);
    let default_key = yaml_key("agent-default-model");
    let current_default = settings.get(&default_key);
    let current_is_ours = current_default
        .and_then(YamlValue::as_mapping)
        .and_then(|m| m.get(&yaml_key("provider")))
        .and_then(YamlValue::as_str)
        .is_some_and(is_managed_provider_id);
    // 首次接管，或客户已在 DSH Web 里切走后又明确切回 U-King：
    // 都把「这一刻的原选择」存成回滚点。连续换 U-King 模型则不覆盖它。
    if !current_is_ours {
        save_dsh_driver_backup(current_default)?;
    } else if load_dsh_driver_backup()?.is_none() {
        // 已经是我们在管、回滚点却没了（信物损坏被清掉，或被手删）。
        // 🔴 这时**不能**存 `current_default` —— 那是我们自己写的那条，
        // 还原时会把「默认模型」指回一个下一秒就被删掉的 route，客户的 DSH 直接瞎掉。
        // 接管前是什么已经无从得知，就如实存「没有用户层选择」：还原时移除我们写的默认，
        // 客户在 DSH Web 里重选一次。丢一个回滚点 < 留一个悬空指针。
        save_dsh_driver_backup(None)?;
    }
    settings.insert(
        default_key,
        serde_yaml::to_value(json!({ "provider": route_id, "model": model }))
            .map_err(|e| format!("生成 DSH 默认模型配置失败: {e}"))?,
    );

    backup_once(&credentials_path);
    backup_once(&settings_path);
    // 先落凭据，再让模型 route 生效；避免 DSH 热加载在两步之间看到一条空 Key route。
    write_yaml_mapping(&credentials_path, &credentials_before, &credentials, "DSH .credentials.yaml", true)?;
    protect_dsh_credentials(&credentials_path)?;
    write_yaml_mapping(&settings_path, &settings_before, &settings, "DSH settings.yaml", false)
}

/// 还原 DSH：只删 U-King 独占 route / credential；客户自己的 provider 和默认选择不碰。
fn reset_dsh() -> Result<(), String> {
    let dir = dsh_dir();
    let settings_path = dir.join("settings.yaml");
    let credentials_path = dir.join(".credentials.yaml");
    let (settings_before, mut settings) = read_yaml_mapping(&settings_path, "DSH settings.yaml")?;
    let (credentials_before, mut credentials) =
        read_yaml_mapping(&credentials_path, "DSH .credentials.yaml")?;

    let mut settings_changed = false;
    let llm_key = yaml_key("llm-pi-ai");
    let providers_key = yaml_key("providers");
    let mut remove_llm = false;
    if let Some(YamlValue::Mapping(llm)) = settings.get_mut(&llm_key) {
        let mut remove_providers = false;
        if let Some(YamlValue::Mapping(providers)) = llm.get_mut(&providers_key) {
            let before = providers.len();
            providers.retain(|key, _| {
                key.as_str()
                    .map(|id| !is_managed_provider_id(id))
                    .unwrap_or(true)
            });
            if providers.len() != before {
                settings_changed = true;
            }
            remove_providers = providers.is_empty();
        }
        if remove_providers {
            llm.remove(&providers_key);
        }
        remove_llm = llm.is_empty();
    }
    if remove_llm {
        settings.remove(&llm_key);
    }

    let backup = load_dsh_driver_backup()?;
    let default_key = yaml_key("agent-default-model");
    let default_is_ours = settings
        .get(&default_key)
        .and_then(YamlValue::as_mapping)
        .and_then(|m| m.get(&yaml_key("provider")))
        .and_then(YamlValue::as_str)
        .is_some_and(is_managed_provider_id);
    if default_is_ours {
        match backup.as_ref().and_then(|b| b.previous_default_yaml.as_deref()) {
            // 同 `load_dsh_driver_backup`：信物里那段 YAML 坏了也不能把整条还原路卡死。
            // 「把 U-King 从 DSH 里摘出去」是客户随时该做得到的事，
            // 回不到接管前的选择是可接受的降级，做不到摘出去不是。
            Some(text) => match serde_yaml::from_str::<YamlValue>(text) {
                Ok(previous) => {
                    settings.insert(default_key.clone(), previous);
                }
                Err(e) => {
                    crate::ulog::write(
                        "providers",
                        &format!("DSH 原默认模型备份损坏，改为移除 U-King 写的默认: {e}"),
                    );
                    settings.remove(&default_key);
                }
            },
            None => {
                settings.remove(&default_key);
            }
        }
        settings_changed = true;
    }

    let credential_changed = credentials.remove(&yaml_key(DSH_UKING_CREDENTIAL)).is_some();
    if settings_changed {
        backup_once(&settings_path);
        write_yaml_mapping(&settings_path, &settings_before, &settings, "DSH settings.yaml", false)?;
    }
    if credential_changed {
        backup_once(&credentials_path);
        write_yaml_mapping(&credentials_path, &credentials_before, &credentials, "DSH .credentials.yaml", true)?;
        protect_dsh_credentials(&credentials_path)?;
    }
    if backup.is_some() {
        std::fs::remove_file(dsh_driver_backup_path())
            .map_err(|e| format!("删除 DSH 回滚信物失败: {e}"))?;
    }
    Ok(())
}

/// 从 DSH 活配置读当前 provider / model / endpoint，供回显和「别自动覆盖」门禁共用。
fn dsh_live_selection() -> (Option<String>, Option<String>, Option<String>) {
    let path = dsh_dir().join("settings.yaml");
    let Ok(text) = std::fs::read_to_string(path) else {
        return (None, None, None);
    };
    let Ok(YamlValue::Mapping(root)) = serde_yaml::from_str::<YamlValue>(&text) else {
        return (None, None, None);
    };
    let selection = root
        .get(&yaml_key("agent-default-model"))
        .and_then(YamlValue::as_mapping);
    let provider = selection
        .and_then(|m| m.get(&yaml_key("provider")))
        .and_then(YamlValue::as_str)
        .map(String::from);
    let model = selection
        .and_then(|m| m.get(&yaml_key("model")))
        .and_then(YamlValue::as_str)
        .map(String::from);
    let base = provider
        .as_deref()
        .filter(|p| is_managed_provider_id(p))
        .and_then(|_| root.get(&yaml_key("llm-pi-ai")))
        .and_then(YamlValue::as_mapping)
        .and_then(|m| m.get(&yaml_key("providers")))
        .and_then(YamlValue::as_mapping)
        .and_then(|m| m.get(&yaml_key(provider.as_deref().unwrap_or_default())))
        .and_then(YamlValue::as_mapping)
        .and_then(|m| m.get(&yaml_key("baseURL")))
        .and_then(YamlValue::as_str)
        .map(String::from);
    (provider, model, base)
}

#[cfg(test)]
mod managed_provider_identity_tests {
    use super::*;

    fn custom(id: &str, name: &str, base: &str) -> ProviderPreset {
        ProviderPreset {
            id: id.into(),
            name: name.into(),
            summary: String::new(),
            openai_base: base.into(),
            anthropic_base: None,
            model: "m".into(),
            small_model: String::new(),
            codex_model: String::new(),
            codex_wire_api: WIRE_API.into(),
            key_url: String::new(),
            key_hint: String::new(),
            builtin_recharge: false,
            recommended: false,
            builtin: false,
            api_key: String::new(),
        }
    }

    #[test]
    fn managed_ids_are_truthful_and_namespaced() {
        assert_eq!(managed_provider_id(&custom("xiapan", "虾盘云", "https://x")), "uking-xiapan");
        assert_eq!(
            managed_provider_id(&custom("custom-openrouter", "OpenRouter", "https://openrouter.ai/api/v1")),
            "uking-openrouter"
        );
        assert_eq!(
            managed_provider_id(&custom("deepseek", "DeepSeek 官方", "https://api.deepseek.com/v1")),
            "uking-deepseek"
        );
        assert!(is_managed_provider_id("uking-managed"), "老 DSH route 也必须被迁移逻辑认领");
        assert!(!is_managed_provider_id("openrouter"), "客户自己的内置 route 不属于我们");
    }

    #[test]
    fn managed_route_round_trips_to_the_uking_provider_id() {
        crate::testsandbox::with_sandbox("managed-route-readback", &[".uking"], |_| {
            let openrouter = custom(
                "custom-openrouter",
                "OpenRouter",
                "https://openrouter.ai/api/v1",
            );
            write_custom_providers(&[openrouter]).unwrap();
            assert_eq!(
                provider_id_from_managed_route("uking-openrouter").as_deref(),
                Some("custom-openrouter")
            );
            assert_eq!(
                provider_id_from_managed_route("uking-deepseek").as_deref(),
                Some("deepseek")
            );
            assert!(provider_id_from_managed_route("openrouter").is_none());
        });
    }

    /// Pi / OpenCode / Crush 共用同一条身份约定：切上游后只留新的 `uking-*`，
    /// 客户自己建的 provider 必须原样保留。Qwen schema 没有 provider id，则迁移它的 envKey。
    #[test]
    fn cli_routes_migrate_from_fake_xiapan_to_real_upstream_without_touching_customer_entries() {
        crate::testsandbox::with_sandbox(
            "managed-provider-routes",
            &[".pi", ".config", ".qwen", ".uking", "LocalAppData"],
            |root| {
                let pi = root.join(".pi").join("agent");
                std::fs::create_dir_all(&pi).unwrap();
                std::fs::write(
                    pi.join("models.json"),
                    r#"{"providers":{"customer":{"baseUrl":"https://customer.test/v1"},"uking-xiapan":{"baseUrl":"https://wrong-old.test/v1"}}}"#,
                )
                .unwrap();
                std::fs::write(
                    pi.join("settings.json"),
                    r#"{"defaultProvider":"uking-xiapan","defaultModel":"old"}"#,
                )
                .unwrap();

                let crush = root.join("LocalAppData").join("crush").join("crush.json");
                std::fs::create_dir_all(crush.parent().unwrap()).unwrap();
                std::fs::write(
                    &crush,
                    r#"{"providers":{"customer":{"name":"mine"},"uking-xiapan":{"name":"wrong old"}},"models":{}}"#,
                )
                .unwrap();

                let oc = root.join(".config").join("opencode").join("opencode.json");
                std::fs::create_dir_all(oc.parent().unwrap()).unwrap();
                std::fs::write(
                    &oc,
                    r#"{"provider":{"customer":{"npm":"x"},"uking-xiapan":{"npm":"old"}},"model":"uking-xiapan/old"}"#,
                )
                .unwrap();

                let qwen = root.join(".qwen").join("settings.json");
                std::fs::create_dir_all(qwen.parent().unwrap()).unwrap();
                std::fs::write(
                    &qwen,
                    r#"{"env":{"UKING_XIAPAN_API_KEY":"old"},"modelProviders":{"openai":[{"id":"old","envKey":"UKING_XIAPAN_API_KEY"},{"id":"mine","envKey":"CUSTOMER_KEY"}]}}"#,
                )
                .unwrap();

                let openrouter = custom(
                    "custom-openrouter",
                    "OpenRouter",
                    "https://openrouter.ai/api/v1",
                );
                apply_pi(&openrouter, "sk-or-test", "stealth/ox-alpha").unwrap();
                apply_crush(&openrouter, "sk-or-test", "stealth/ox-alpha").unwrap();
                apply_opencode(&openrouter, "sk-or-test", "stealth/ox-alpha").unwrap();
                apply_qwen(&openrouter, "sk-or-test", "stealth/ox-alpha").unwrap();

                let pm: Value = serde_json::from_str(&std::fs::read_to_string(pi.join("models.json")).unwrap()).unwrap();
                let ps: Value = serde_json::from_str(&std::fs::read_to_string(pi.join("settings.json")).unwrap()).unwrap();
                assert!(pm["providers"]["customer"].is_object());
                assert!(pm["providers"]["uking-xiapan"].is_null());
                assert_eq!(pm["providers"]["uking-openrouter"]["baseUrl"], "https://openrouter.ai/api/v1");
                assert_eq!(ps["defaultProvider"], "uking-openrouter");

                let cv: Value = serde_json::from_str(&std::fs::read_to_string(&crush).unwrap()).unwrap();
                assert!(cv["providers"]["customer"].is_object());
                assert!(cv["providers"]["uking-xiapan"].is_null());
                assert_eq!(cv["models"]["large"]["provider"], "uking-openrouter");
                assert_eq!(cv["models"]["small"]["provider"], "uking-openrouter");

                let ov: Value = serde_json::from_str(&std::fs::read_to_string(&oc).unwrap()).unwrap();
                assert!(ov["provider"]["customer"].is_object());
                assert!(ov["provider"]["uking-xiapan"].is_null());
                assert_eq!(ov["model"], "uking-openrouter/stealth/ox-alpha");

                let qv: Value = serde_json::from_str(&std::fs::read_to_string(&qwen).unwrap()).unwrap();
                assert!(qv["env"].get(LEGACY_UKING_ENV_KEY).is_none());
                assert_eq!(qv["env"][UKING_ENV_KEY], "sk-or-test");
                let entries = qv["modelProviders"]["openai"].as_array().unwrap();
                assert!(entries.iter().any(|e| e["envKey"] == "CUSTOMER_KEY"));
                assert_eq!(entries.iter().filter(|e| e["envKey"] == UKING_ENV_KEY).count(), 1);

                // 再切 DeepSeek：OpenRouter 托管槽必须被替换，不能越积越多。
                let deepseek = custom("deepseek", "DeepSeek 官方", "https://api.deepseek.com/v1");
                apply_pi(&deepseek, "sk-ds", "deepseek-chat").unwrap();
                apply_crush(&deepseek, "sk-ds", "deepseek-chat").unwrap();
                apply_opencode(&deepseek, "sk-ds", "deepseek-chat").unwrap();
                let pm2: Value = serde_json::from_str(&std::fs::read_to_string(pi.join("models.json")).unwrap()).unwrap();
                let cv2: Value = serde_json::from_str(&std::fs::read_to_string(&crush).unwrap()).unwrap();
                let ov2: Value = serde_json::from_str(&std::fs::read_to_string(&oc).unwrap()).unwrap();
                assert!(pm2["providers"]["uking-openrouter"].is_null());
                assert!(pm2["providers"]["uking-deepseek"].is_object());
                assert!(cv2["providers"]["uking-openrouter"].is_null());
                assert_eq!(cv2["models"]["large"]["provider"], "uking-deepseek");
                assert!(ov2["provider"]["uking-openrouter"].is_null());
                assert_eq!(ov2["model"], "uking-deepseek/deepseek-chat");
            },
        );
    }

    #[test]
    fn clawx_and_openclaw_switch_managed_identity_without_orphans() {
        crate::testsandbox::with_sandbox(
            "managed-clawx-routes",
            &["ClawX", ".openclaw", ".uking"],
            |root| {
                let cfg = root.join("ClawX").join("clawx-providers.json");
                std::fs::create_dir_all(cfg.parent().unwrap()).unwrap();
                std::fs::write(
                    &cfg,
                    r#"{"schemaVersion":2,"providerAccounts":{"customer":{"id":"customer","baseUrl":"https://customer.test/v1","updatedAt":"2026-01-01T00:00:00.000Z","createdAt":"2026-01-01T00:00:00.000Z"}},"apiKeys":{"customer":"mine"},"providerSecrets":{},"defaultProvider":"customer"}"#,
                )
                .unwrap();

                let openrouter = custom(
                    "custom-openrouter",
                    "OpenRouter",
                    "https://openrouter.ai/api/v1",
                );
                apply_clawx(&openrouter, "sk-or", "stealth/ox-alpha").unwrap();
                let v: Value = serde_json::from_str(&std::fs::read_to_string(&cfg).unwrap()).unwrap();
                assert_eq!(v["defaultProvider"], "uking-openrouter");
                assert!(v["providerAccounts"]["customer"].is_object());
                assert_eq!(v["providerAccounts"]["uking-openrouter"]["label"], "OpenRouter");
                let oc_path = root.join(".openclaw").join("openclaw.json");
                let oc: Value = serde_json::from_str(&std::fs::read_to_string(&oc_path).unwrap()).unwrap();
                assert_eq!(
                    oc["agents"]["defaults"]["model"]["primary"],
                    "custom-ukingope/stealth/ox-alpha"
                );

                let deepseek = custom("deepseek", "DeepSeek 官方", "https://api.deepseek.com/v1");
                apply_clawx(&deepseek, "sk-ds", "deepseek-chat").unwrap();
                let v2: Value = serde_json::from_str(&std::fs::read_to_string(&cfg).unwrap()).unwrap();
                assert_eq!(v2["defaultProvider"], "uking-deepseek");
                assert!(v2["providerAccounts"]["uking-openrouter"].is_null());
                assert!(v2["providerAccounts"]["customer"].is_object());
                let oc2: Value = serde_json::from_str(&std::fs::read_to_string(&oc_path).unwrap()).unwrap();
                assert!(oc2["models"]["providers"]["custom-ukingope"].is_null());
                assert_eq!(oc2["agents"]["defaults"]["model"]["primary"], "custom-ukingdee/deepseek-chat");

                reset_clawx().unwrap();
                let v3: Value = serde_json::from_str(&std::fs::read_to_string(&cfg).unwrap()).unwrap();
                assert!(v3["providerAccounts"]["uking-deepseek"].is_null());
                assert!(v3["providerAccounts"]["customer"].is_object());
                assert_eq!(v3["defaultProvider"], "customer");
            },
        );
    }
}

#[cfg(test)]
mod pi_provider_tests {
    use super::*;

    fn preset(base: &str) -> ProviderPreset {
        ProviderPreset {
            id: "t".into(),
            name: "测试供应商".into(),
            summary: String::new(),
            openai_base: base.into(),
            anthropic_base: None,
            model: String::new(),
            small_model: String::new(),
            codex_model: String::new(),
            codex_wire_api: WIRE_API.into(),
            key_url: String::new(),
            key_hint: String::new(),
            builtin_recharge: false,
            recommended: false,
            builtin: false,
            api_key: String::new(),
        }
    }

    /// 🔴 客户实锤（2026-08-24）：「设置了 pi 用 deepseek，启动 pi 发现还是 kimi」。
    ///
    /// 真因是 settings.json 只写了 `defaultModel = "<provider>/<model>"`（那是命令行
    /// `--model` 的语法）而**从不写 `defaultProvider`**，于是机器上残留的
    /// `defaultProvider: "openrouter"` 一直说了算。四组变异实测见 `apply_pi` 函数头。
    ///
    /// 这条用例守三件事，缺一条就会退回那个「GUI 报成功、pi 跑别人」的形状：
    ///   ① `defaultProvider` 必须被写成我们的 provider（**这条是原 bug 的直接判据**）
    ///   ② `defaultModel` 必须是**裸 id**，不带 `provider/` 前缀
    ///   ③ baseUrl 要 trim（客户粘贴进来的自定义端点带前导空格，实测已落盘）
    #[test]
    fn apply_pi_writes_both_default_provider_and_bare_model() {
        crate::testsandbox::with_sandbox("pi-default-model", &[".pi", ".uking"], |root| {
            // 造出「客户机上的残留」：defaultProvider 指着别人。修好之前它会一直赢。
            let dir = root.join(".pi").join("agent");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join("settings.json"),
                r#"{"defaultProvider":"openrouter","theme":"dark"}"#,
            )
            .unwrap();

            apply_pi(
                &preset("  https://example.test/v1  "),
                "sk-test",
                "deepseek-v4-flash",
            )
            .expect("apply_pi 失败");

            let s: Value = serde_json::from_str(
                &std::fs::read_to_string(dir.join("settings.json")).unwrap(),
            )
            .unwrap();
            // ① 抢过决定权
            assert_eq!(
                s["defaultProvider"].as_str(),
                Some("uking-t"),
                "没写 defaultProvider → 残留的 openrouter 说了算，pi 会跑别人的模型"
            );
            // ② 裸 id，不是 provider/id
            assert_eq!(
                s["defaultModel"].as_str(),
                Some("deepseek-v4-flash"),
                "defaultModel 必须是裸 model id（pi docs/settings.md），带前缀 pi 匹配不上"
            );
            // 用户自己的键不许被顺手抹掉
            assert_eq!(s["theme"].as_str(), Some("dark"));

            // ③ baseUrl trim
            let m: Value =
                serde_json::from_str(&std::fs::read_to_string(dir.join("models.json")).unwrap())
                    .unwrap();
            assert_eq!(
                m["providers"]["uking-t"]["baseUrl"].as_str(),
                Some("https://example.test/v1"),
                "baseUrl 没 trim —— 客户粘贴的端点带空格会整条请求发不出去"
            );
        });
    }

    /// 🔴 `opencode.jsonc` 压过 `opencode.json` 的顶层 `model`（2026-08-24 沙箱实测
    /// `opencode debug config`：两份文件各钉一个 model，解析出来的是 jsonc 那个）。
    /// 只写 json 就是「写了个不生效的字段」—— 用户机器上恰好两份都有，jsonc 里钉着
    /// `openrouter/stealth/ox-alpha`，于是 GUI 报「已切到 DeepSeek」而 opencode 照跑 ox。
    #[test]
    fn apply_opencode_also_fixes_the_jsonc_that_overrides_it() {
        crate::testsandbox::with_sandbox("opencode-jsonc", &[".uking"], |root| {
            let dir = root.join(".config").join("opencode");
            std::fs::create_dir_all(&dir).unwrap();
            // 用户手里那份：钉着别人家的模型，且**没有注释**（严格 JSON 能解析）。
            std::fs::write(
                dir.join("opencode.jsonc"),
                r#"{"model":"openrouter/stealth/ox-alpha","provider":{"openrouter":{"npm":"x"}}}"#,
            )
            .unwrap();

            apply_opencode(&preset("https://example.test/v1"), "sk-test", "deepseek-v4-flash")
                .expect("apply_opencode 失败");

            let c: Value = serde_json::from_str(
                &std::fs::read_to_string(dir.join("opencode.jsonc")).unwrap(),
            )
            .unwrap();
            assert_eq!(
                c["model"].as_str(),
                Some("uking-t/deepseek-v4-flash"),
                "jsonc 的 model 没对齐 → 它会压过 json，切换等于没切"
            );
            // 用户在 jsonc 里的其它内容不许被抹掉
            assert!(c["provider"]["openrouter"].is_object(), "动 model 时把用户的 provider 弄丢了");
        });
    }

    /// 🔴 `.jsonc` 可以带注释，而我们只有严格 JSON 解析器。**解析不动就不许回写** ——
    /// 强行 `to_string_pretty` 会把用户的注释全部抹掉（宪法 10）。这时正确的行为是
    /// **报错并指出是哪个文件在压着**，而不是报成功让用户自己去猜为什么没生效。
    #[test]
    fn apply_opencode_refuses_to_clobber_a_commented_jsonc() {
        crate::testsandbox::with_sandbox("opencode-jsonc-comment", &[".uking"], |root| {
            let dir = root.join(".config").join("opencode");
            std::fs::create_dir_all(&dir).unwrap();
            let original = "{\n  // 我自己配的，别动\n  \"model\": \"openrouter/stealth/ox-alpha\"\n}";
            std::fs::write(dir.join("opencode.jsonc"), original).unwrap();

            let err = apply_opencode(&preset("https://example.test/v1"), "sk-test", "m1")
                .expect_err("带注释的 jsonc 压着我们，必须报错而不是假装成功");
            assert!(
                err.contains("opencode.jsonc"),
                "报错里必须点名是哪个文件在压着，否则用户无从下手：{err}"
            );
            assert_eq!(
                std::fs::read_to_string(dir.join("opencode.jsonc")).unwrap(),
                original,
                "带注释的文件被我们改写了 —— 注释就是这么没的"
            );
        });
    }

    /// 没有 jsonc 就没有冲突：不许凭空造一个出来。
    #[test]
    fn apply_opencode_does_not_create_a_jsonc_out_of_nothing() {
        crate::testsandbox::with_sandbox("opencode-no-jsonc", &[".uking"], |root| {
            apply_opencode(&preset("https://example.test/v1"), "sk-test", "m1").unwrap();
            let dir = root.join(".config").join("opencode");
            assert!(dir.join("opencode.json").exists(), "该写的 json 没写");
            assert!(!dir.join("opencode.jsonc").exists(), "凭空造了一个 jsonc");
        });
    }

    /// 🔴 **存量脏数据的自愈路径**：已经带着前导空格躺在 providers.json 里的那些，
    /// 读出来就该是干净的。只在 `save` 侧 trim 救不了老机器 —— 用户不会因为我们发了
    /// 新版就重新粘贴一遍端点，而 `" https://x"` 和 `"https://x"` 在界面上长得一模一样。
    #[test]
    fn read_custom_providers_heals_pasted_whitespace() {
        crate::testsandbox::with_sandbox("read-trim", &[".uking"], |root| {
            let p = root.join(".uking").join("providers.json");
            // 直接写脏文件，**不走 save_custom_provider** —— 走了就被 save 侧的 trim 洗掉，
            // 这条用例就永远测不到它想测的东西（会变成一条恒真的重言式）。
            std::fs::write(
                &p,
                r#"[{"id":"custom-x","name":" 带空格的名字 ","summary":"",
                    "openai_base":" https://x.test/v1 ","anthropic_base":null,
                    "model":" m1 ","small_model":"","codex_model":"","codex_wire_api":"responses",
                    "key_url":"","key_hint":"","builtin_recharge":false,"recommended":false,
                    "builtin":false,"api_key":" sk-dirty "}]"#,
            )
            .unwrap();

            let list = read_custom_providers();
            let x = list.iter().find(|q| q.id == "custom-x").expect("没读到那条自定义供应商");
            assert_eq!(x.openai_base, "https://x.test/v1", "端点没自愈 —— 老机器会一直配出打不通的驱动");
            assert_eq!(x.name, "带空格的名字");
            assert_eq!(x.model, "m1");
            assert_eq!(x.api_key, "sk-dirty");
            // 只读不写：不许因为一次读取就产生用户没要求的落盘（宪法 10）
            assert!(
                std::fs::read_to_string(&p).unwrap().contains(" https://x.test/v1 "),
                "读取顺手改写了用户的文件"
            );
        });
    }

    /// 回验的核心断言：**回读出来的必须是「工具真正会用的那个」，不是「我们写进去的那个」。**
    ///
    /// 这条用例的价值全在下面那段「先造一个坏状态」里 —— 如果 `effective_config("pi")`
    /// 图省事去读 `models.json` 里我们自己写的 provider，它会报「已生效」，
    /// 而 pi 实际跑的是 `defaultProvider` 指的那个。**回验读错地方 = 回验白做**，
    /// 且比没有回验更坏（它会给出一个可信的错结论）。
    #[test]
    fn effective_config_pi_reports_what_pi_would_actually_run() {
        crate::testsandbox::with_sandbox("eff-pi", &[".pi", ".uking"], |root| {
            let dir = root.join(".pi").join("agent");
            std::fs::create_dir_all(&dir).unwrap();
            // 造出客户机上那个真实的坏状态：models.json 里我们的 provider 配得好好的，
            // 但 defaultProvider 指着别人 —— pi 会跑别人那个。
            std::fs::write(
                dir.join("models.json"),
                r#"{"providers":{"uking-xiapan":{"baseUrl":"https://ours.test/v1"},
                    "openrouter":{"baseUrl":"https://openrouter.ai/api/v1"}}}"#,
            )
            .unwrap();
            std::fs::write(
                dir.join("settings.json"),
                r#"{"defaultProvider":"openrouter","defaultModel":"moonshotai/kimi-k2.6"}"#,
            )
            .unwrap();

            let e = effective_config("pi");
            assert!(e.readable, "pi 是有回读路径的，不该报「不知道」");
            assert_eq!(
                e.provider_key.as_deref(),
                Some("openrouter"),
                "回读必须跟 pi 同口径看 defaultProvider —— 读成我们自己写的那个就等于没回验"
            );
            assert_eq!(e.model.as_deref(), Some("moonshotai/kimi-k2.6"));
            assert_eq!(e.base_url.as_deref(), Some("https://openrouter.ai/api/v1"));

            // 修好之后必须翻转（否则这条断言可能恒真）
            apply_pi(&preset("https://ours.test/v1"), "sk", "deepseek-v4-flash").unwrap();
            let e2 = effective_config("pi");
            assert_eq!(e2.provider_key.as_deref(), Some("uking-t"));
            assert_eq!(e2.model.as_deref(), Some("deepseek-v4-flash"));
        });
    }

    /// opencode：`jsonc` 压着 `json` 时，回读必须报**被压着的那个真相**，
    /// 并点名是哪个文件在压 —— 这正是 GUI 要显示给客户看的那句话。
    #[test]
    fn effective_config_opencode_reports_the_jsonc_override() {
        crate::testsandbox::with_sandbox("eff-oc", &[".uking"], |root| {
            let dir = root.join(".config").join("opencode");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join("opencode.json"),
                r#"{"model":"uking-xiapan/deepseek-v4-flash",
                    "provider":{"uking-xiapan":{"options":{"baseURL":"https://ours.test/v1"}}}}"#,
            )
            .unwrap();
            std::fs::write(dir.join("opencode.jsonc"), r#"{"model":"openrouter/ox-alpha"}"#).unwrap();

            let e = effective_config("opencode");
            assert!(e.readable);
            assert_eq!(e.provider_key.as_deref(), Some("openrouter"), "没看 jsonc = 回验白做");
            assert_eq!(e.model.as_deref(), Some("ox-alpha"));
            assert!(
                e.overridden_by.as_deref().is_some_and(|p| p.ends_with("opencode.jsonc")),
                "必须点名压着它的文件，否则客户无从下手：{:?}",
                e.overridden_by
            );

            // 🔴 **两份一致之后必须不再报警告。** 第一版判据是「jsonc 里有 model 就算被压」，
            // 于是切换成功、已经对齐之后界面照样弹橙色 —— 一个正常状态下长期亮着的警告，
            // 用户两天就学会无视它，等它真该响的时候也没人看了。
            std::fs::write(
                dir.join("opencode.jsonc"),
                r#"{"model":"uking-xiapan/deepseek-v4-flash"}"#,
            )
            .unwrap();
            let e1 = effective_config("opencode");
            assert_eq!(e1.provider_key.as_deref(), Some(LEGACY_UKING_PROVIDER_ID));
            assert!(e1.overridden_by.is_none(), "两份已经一致，不该再报「被压着」");

            // 没有 jsonc 时同样不许凭空报「被压着」
            std::fs::remove_file(dir.join("opencode.jsonc")).unwrap();
            let e2 = effective_config("opencode");
            assert_eq!(e2.provider_key.as_deref(), Some(LEGACY_UKING_PROVIDER_ID));
            assert!(e2.overridden_by.is_none(), "没有 jsonc 却报了「被压着」");
        });
    }

    /// 🔴 **带注释的 jsonc = 我们看不见 → 必须报「不知道」，不许猜成「生效了」。**
    /// 空结果有两义（没查 / 查了没有），把「没查」渲染成绿勾就是又一份假绿。
    #[test]
    fn effective_config_opencode_says_unknown_when_it_cannot_see() {
        crate::testsandbox::with_sandbox("eff-oc-blind", &[".uking"], |root| {
            let dir = root.join(".config").join("opencode");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("opencode.json"), r#"{"model":"uking-xiapan/m"}"#).unwrap();
            std::fs::write(dir.join("opencode.jsonc"), "{\n // 注释\n \"model\": \"x/y\"\n}").unwrap();

            let e = effective_config("opencode");
            assert!(!e.readable, "解析不动却报了 readable —— 这就是「报告是对的，世界是坏的」");
            assert!(e.overridden_by.is_some(), "至少要说清是哪个文件挡住了视线");
        });
    }

    /// 没有回读路径的工具一律报「不知道」，绝不返回一个空的、看着像「没配置」的结论。
    /// （clawx 现仍无独立回读，driver_status 单独回显；qwen/crush 的回读 2026-08-27
    /// 已补齐——ee58e3f 为钱包同步加的，ai_checkup 也吃同一份。所以名单只剩 clawx。）
    #[test]
    fn effective_config_admits_it_has_no_readback_path() {
        crate::testsandbox::with_sandbox("eff-unknown", &[".uking"], |_| {
            for t in ["clawx", "什么鬼"] {
                assert!(!effective_config(t).readable, "{t} 没有回读路径却报了 readable");
            }
        });
    }

    /// 还原要把两个键一起摘掉。只摘 `defaultModel` 会留下一条指着已删 provider 的
    /// `defaultProvider`，pi 起手即报错 —— 比不还原更糟。
    #[test]
    fn reset_pi_removes_both_keys_it_owns() {
        crate::testsandbox::with_sandbox("pi-reset", &[".pi", ".uking"], |root| {
            let dir = root.join(".pi").join("agent");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("settings.json"), r#"{"theme":"dark"}"#).unwrap();

            apply_pi(&preset("https://example.test/v1"), "sk-test", "m1").unwrap();
            reset_pi().expect("reset_pi 失败");

            let s: Value = serde_json::from_str(
                &std::fs::read_to_string(dir.join("settings.json")).unwrap(),
            )
            .unwrap();
            assert!(s.get("defaultProvider").is_none(), "残留 defaultProvider");
            assert!(s.get("defaultModel").is_none(), "残留 defaultModel");
            assert_eq!(s["theme"].as_str(), Some("dark"), "还原不许动用户自己的键");
        });
    }
}

#[cfg(test)]
mod dsh_provider_tests {
    use super::*;

    fn at<'a>(root: &'a YamlValue, path: &[&str]) -> Option<&'a YamlValue> {
        let mut current = root;
        for key in path {
            current = current.as_mapping()?.get(&yaml_key(key))?;
        }
        Some(current)
    }

    /// 🔴 回滚信物损坏时，接管和还原**都必须照跑** —— 否则唯一的出路是让客户手删文件。
    ///
    /// 原来 `load_dsh_driver_backup` 把「损坏」当硬错往上抛，而全代码里唯一删除该文件的地方
    /// 在还原那个 `?` 的下游：接管失败 → 还原也失败 → 清除代码永远走不到。死锁，且无声。
    /// 空文件就能触发（`fs::read` 成功、`from_slice(&[])` 必然 EOF），
    /// 一次被打断的 `atomic_write` 即可造出来。
    #[test]
    fn corrupt_dsh_backup_never_locks_out_takeover_or_reset() {
        crate::testsandbox::with_sandbox("dsh-corrupt-backup", &[".dsh", ".uking"], |root| {
            // ulog 认的是 USERPROFILE/HOME（不认 UKING_TEST_HOME），不改的话这条用例
            // 会把日志写进开发机真实的 ~/.uking/logs。沙箱已存档这两个变量，出去自动还原。
            std::env::set_var("USERPROFILE", root);
            std::env::set_var("HOME", root);

            let settings_path = root.join(".dsh").join("settings.yaml");
            std::fs::write(
                &settings_path,
                "agent-default-model:\n  provider: customer-gateway\n  model: customer-model\n",
            )
            .unwrap();

            let p = builtin_providers().into_iter().find(|p| p.id == "xiapan").unwrap();
            // 先正常接管一次 —— **信物必须是在「已经是我们在管」之后才损坏的**。
            // 第一版用例把文件在接管前就写坏，结果 `!current_is_ours ||` 短路，
            // `load` 压根没被调用：把修好的代码改回旧写法，用例照样绿（变异验证当场抓到）。
            // 死锁的真实现场只有一个：接管完成 → 信物被打断的写留成空文件 → 从此换模型和还原都失败。
            apply_dsh(&p, "sk-device-secret", "deepseek-v4-flash").unwrap();
            let backup = dsh_driver_backup_path();
            assert!(backup.exists(), "前提：接管后该有回滚点");
            // 0 字节 = atomic_write 被打断后盘上的样子。
            std::fs::write(&backup, b"").unwrap();

            apply_dsh(&p, "sk-device-secret-2", "deepseek-v4-pro").expect("信物损坏不该挡住换模型");
            reset_dsh().expect("信物损坏不该挡住还原");

            // 自愈：坏信物被清掉，客户不必手删。
            assert!(!backup.exists(), "损坏的信物必须被清掉，不能留给客户手删");

            // 降级要体面：接管前那个选择已经无从得知（信物没了），但**绝不能留下悬空默认** ——
            // 指着一个刚被删掉的 uking-managed route，比没有默认更坏。
            let restored: YamlValue =
                serde_yaml::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
            assert!(
                at(&restored, &["llm-pi-ai", "providers", LEGACY_UKING_PROVIDER_ID]).is_none(),
                "还原必须把 U-King 的 route 删干净：{restored:?}",
            );
            assert_ne!(
                at(&restored, &["agent-default-model", "provider"]).and_then(YamlValue::as_str),
                Some(LEGACY_UKING_PROVIDER_ID),
                "还原后不许留下指向已删 route 的默认模型：{restored:?}",
            );
        });
    }

    #[test]
    fn dsh_apply_and_reset_preserve_customer_state_and_keep_secret_out_of_settings() {
        crate::testsandbox::with_sandbox("dsh-driver-roundtrip", &[".dsh", ".uking"], |root| {
            let dsh = root.join(".dsh");
            let settings_path = dsh.join("settings.yaml");
            let credentials_path = dsh.join(".credentials.yaml");
            std::fs::write(
                &settings_path,
                "locale:\n  language: zh-CN\nllm-pi-ai:\n  providers:\n    customer-gateway:\n      apiKeyEnv: CUSTOMER_KEY\n      api: openai-completions\n      baseURL: https://customer.example/v1\n      models:\n        - id: customer-model\nagent-default-model:\n  provider: customer-gateway\n  model: customer-model\n  reasoningEffort: high\n",
            )
            .unwrap();
            std::fs::write(&credentials_path, "CUSTOMER_KEY: sk-customer\n").unwrap();

            let p = builtin_providers().into_iter().find(|p| p.id == "xiapan").unwrap();
            apply_dsh(&p, "sk-device-secret", "deepseek-v4-flash").unwrap();

            let settings_text = std::fs::read_to_string(&settings_path).unwrap();
            let settings: YamlValue = serde_yaml::from_str(&settings_text).unwrap();
            assert!(!settings_text.contains("sk-device-secret"), "Key 绝不能进 settings.yaml");
            assert_eq!(
                at(&settings, &["locale", "language"]).and_then(YamlValue::as_str),
                Some("zh-CN"),
            );
            assert_eq!(
                at(&settings, &["llm-pi-ai", "providers", "customer-gateway", "baseURL"])
                    .and_then(YamlValue::as_str),
                Some("https://customer.example/v1"),
                "客户原 provider 不许丢",
            );
            assert_eq!(
                at(&settings, &["llm-pi-ai", "providers", LEGACY_UKING_PROVIDER_ID, "baseURL"])
                    .and_then(YamlValue::as_str),
                Some("https://api.u-claw.org.cn/v1"),
            );
            assert_eq!(
                at(&settings, &["agent-default-model", "provider"]).and_then(YamlValue::as_str),
                Some(LEGACY_UKING_PROVIDER_ID),
            );
            let credentials: YamlValue =
                serde_yaml::from_str(&std::fs::read_to_string(&credentials_path).unwrap()).unwrap();
            assert_eq!(
                at(&credentials, &["CUSTOMER_KEY"]).and_then(YamlValue::as_str),
                Some("sk-customer"),
            );
            assert_eq!(
                at(&credentials, &[DSH_UKING_CREDENTIAL]).and_then(YamlValue::as_str),
                Some("sk-device-secret"),
            );

            // 在 U-King 内换模型不许覆盖首次接管前的回滚点。
            apply_dsh(&p, "sk-device-secret-2", "deepseek-v4-pro").unwrap();
            reset_dsh().unwrap();

            let restored: YamlValue =
                serde_yaml::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
            assert_eq!(
                at(&restored, &["agent-default-model", "provider"]).and_then(YamlValue::as_str),
                Some("customer-gateway"),
            );
            assert_eq!(
                at(&restored, &["agent-default-model", "model"]).and_then(YamlValue::as_str),
                Some("customer-model"),
            );
            assert_eq!(
                at(&restored, &["agent-default-model", "reasoningEffort"]).and_then(YamlValue::as_str),
                Some("high"),
            );
            assert!(
                at(&restored, &["llm-pi-ai", "providers", LEGACY_UKING_PROVIDER_ID]).is_none(),
                "还原后只该删 U-King route",
            );
            assert!(
                at(&restored, &["llm-pi-ai", "providers", "customer-gateway"]).is_some(),
                "还原不许误删客户 route",
            );
            let restored_credentials: YamlValue =
                serde_yaml::from_str(&std::fs::read_to_string(&credentials_path).unwrap()).unwrap();
            assert!(at(&restored_credentials, &[DSH_UKING_CREDENTIAL]).is_none());
            assert_eq!(
                at(&restored_credentials, &["CUSTOMER_KEY"]).and_then(YamlValue::as_str),
                Some("sk-customer"),
            );
            assert!(!dsh_driver_backup_path().exists(), "成功还原后回滚信物要清掉");
        });
    }

    #[test]
    fn dsh_route_tracks_real_upstream_and_replaces_legacy_managed_route() {
        crate::testsandbox::with_sandbox("dsh-real-provider-id", &[".dsh", ".uking"], |root| {
            let settings_path = root.join(".dsh").join("settings.yaml");
            std::fs::write(
                &settings_path,
                "llm-pi-ai:\n  providers:\n    customer:\n      baseURL: https://customer.test/v1\n    uking-managed:\n      baseURL: https://legacy.test/v1\nagent-default-model:\n  provider: uking-managed\n  model: old\n",
            )
            .unwrap();
            let openrouter = ProviderPreset {
                id: "custom-openrouter".into(),
                name: "OpenRouter".into(),
                openai_base: "https://openrouter.ai/api/v1".into(),
                model: "stealth/ox-alpha".into(),
                ..builtin_providers().into_iter().find(|p| p.id == "xiapan").unwrap()
            };
            apply_dsh(&openrouter, "sk-or", "stealth/ox-alpha").unwrap();
            let settings: YamlValue =
                serde_yaml::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
            assert!(at(&settings, &["llm-pi-ai", "providers", "customer"]).is_some());
            assert!(at(&settings, &["llm-pi-ai", "providers", "uking-managed"]).is_none());
            assert_eq!(
                at(&settings, &["agent-default-model", "provider"]).and_then(YamlValue::as_str),
                Some("uking-openrouter")
            );
            let live = dsh_live_selection();
            assert_eq!(live.0.as_deref(), Some("uking-openrouter"));
            assert_eq!(live.2.as_deref(), Some("https://openrouter.ai/api/v1"));
        });
    }

    #[test]
    fn dsh_invalid_yaml_is_refused_without_overwrite() {
        crate::testsandbox::with_sandbox("dsh-driver-invalid", &[".dsh", ".uking"], |root| {
            let path = root.join(".dsh").join("settings.yaml");
            let broken = b"llm-pi-ai: [unterminated\n";
            std::fs::write(&path, broken).unwrap();
            let p = builtin_providers().into_iter().find(|p| p.id == "xiapan").unwrap();
            let err = apply_dsh(&p, "sk-secret", "deepseek-v4-flash").unwrap_err();
            assert!(err.contains("不是合法 YAML"));
            assert_eq!(std::fs::read(&path).unwrap(), broken, "损坏文档一个字节都不许覆盖");
            assert!(!root.join(".dsh").join(".credentials.yaml").exists());
            assert!(!dsh_driver_backup_path().exists());
        });
    }
}

// ============================================================
//  Qwen Code / Crush（2026-08-03 上架的两个 CLI）
// ------------------------------------------------------------
// 两条路径都是**先在本机跑通、拿到权威配置形状之后**才写的代码：
//   · Qwen：形状取自它 npm 包里自带的 `qc-helper/docs/configuration/auth.md`
//   · Crush：全局配置目录由 `crush dirs` 自己吐出来（Windows 上也是 ~/.config/crush）
// 这一条不是流程洁癖 —— Hermes 当年就是照 `~/.hermes` 猜路径，真身在
// `%LOCALAPPDATA%\hermes`，写了半年都没生效。宁可多跑一条命令问它自己。
// ============================================================

/// Qwen Code 配置文件：`~/.qwen/settings.json`（支持 UKING_TEST_HOME 沙箱）。
fn qwen_settings_path() -> PathBuf {
    config_home().join(".qwen").join("settings.json")
}

/// Crush 全局配置：`~/.config/crush/crush.json`。
/// **Windows 上也是这个路径**（`crush dirs` 实测吐出 `C:\Users\<u>\.config\crush`），
/// 不是 `%APPDATA%\crush` —— 按 Windows 惯例猜会猜错。
/// Crush 配置文件。
///
/// **Windows 上是 `%LOCALAPPDATA%\crush\crush.json`，不是 XDG 的 `~/.config/crush`。**
/// 2026-08-04 实测（crush v0.88）：真实机器上有内容的是前者（providers.json 都 400KB 了），
/// 我们一直在写的后者是个没人读的空目录 —— 于是「一键配好全部」把 Crush 报成已配置，
/// 客户一跑就是 `unauthorized`，还以为是 Key 的问题。
///
/// 定位它花的那步值得记下来：把 base_url 改成死端口，错误**一个字都没变** ——
/// 这才排除了「格式写错」的假设（宪法第 7 条：先拿到能区分假设的决定性测试）。
/// 配置内容原封不动挪到 LOCALAPPDATA 就通了，证明格式一直是对的。
///
/// 同 Hermes 踩过的坑（写了半年 `~/.hermes`，真身在 `%LOCALAPPDATA%\hermes`）。
/// 沙箱路径特意做成**同构**（`<沙箱>/LocalAppData/crush/`）而不是图省事复用 `.config` ——
/// 沙箱结构跟真实不一样，就正是这次没测出来的原因。
fn crush_config_path() -> PathBuf {
    #[cfg(windows)]
    {
        return local_app_data().join("crush").join("crush.json");
    }
    #[cfg(not(windows))]
    {
        config_home().join(".config").join("crush").join("crush.json")
    }
}

/// 老版本写错的位置。只用于 reset 时顺手清掉存量客户机上那份废配置 ——
/// 它没人读，但留着会让「U-King 到底动过哪些文件」对不上账。
#[cfg(windows)]
fn crush_legacy_config_path() -> PathBuf {
    config_home().join(".config").join("crush").join("crush.json")
}

/// `%LOCALAPPDATA%`（沙箱下重定向到 `<沙箱>/LocalAppData`，保持与真实同构）。
#[cfg(windows)]
fn local_app_data() -> PathBuf {
    if let Ok(t) = std::env::var("UKING_TEST_HOME") {
        if !t.trim().is_empty() {
            return PathBuf::from(t).join("LocalAppData");
        }
    }
    std::env::var("LOCALAPPDATA")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| config_home().join("AppData").join("Local"))
}

/// U-King 管理的 provider 使用独占命名空间，但后半段必须如实反映真实上游。
///
/// 旧实现把所有供应商都塞进 `uking-xiapan`：请求虽然会按 base URL 发到 OpenRouter，
/// pi / OpenCode / Crush 的会话元数据却会永久记成「虾盘云」。这不只是文案问题——两个
/// 不同上游共用一个身份，也让诊断、用量归因和回滚都失去语义。
///
/// `ProviderPreset.id` 是 U-King 内部稳定 id；画廊导入项带 `custom-` 前缀，落到外部工具时
/// 去掉它，再加我们自己的 `uking-` 命名空间：
///   xiapan / custom-openrouter / deepseek → uking-xiapan / uking-openrouter / uking-deepseek
/// 这样既不占用 Pi 内置的 `openrouter`（不覆盖客户手配），又不会再冒充虾盘云。
fn managed_provider_id(p: &ProviderPreset) -> String {
    let raw = p.id.trim().strip_prefix("custom-").unwrap_or(p.id.trim());
    let raw = raw.strip_prefix("uking-").unwrap_or(raw);
    format!("uking-{}", slugify(raw))
}

/// `uking-` 是 U-King 在外部工具配置里的保留命名空间。reset / 切换只清这个空间，
/// 不碰 openrouter / deepseek / customer-gateway 等客户自己的 provider。
fn is_managed_provider_id(id: &str) -> bool {
    id.starts_with("uking-")
}

/// 把外部工具里的 `uking-*` route 反查回 U-King 内部 provider id。
/// 例如 `uking-openrouter` → `custom-openrouter`；用定义表反查，不靠猜 `custom-`
/// 前缀，因为内置 `deepseek` 和用户自定义 id 的命名规则不同。
fn provider_id_from_managed_route(route: &str) -> Option<String> {
    all_providers()
        .into_iter()
        .find(|p| managed_provider_id(p) == route)
        .map(|p| p.id)
}

#[cfg(test)]
const LEGACY_UKING_PROVIDER_ID: &str = "uking-xiapan";
const LEGACY_UKING_ENV_KEY: &str = "UKING_XIAPAN_API_KEY";
const UKING_ENV_KEY: &str = "UKING_MANAGED_API_KEY";

/// 把驱动写进 Qwen Code 的 `~/.qwen/settings.json`。
///
/// 只改我们这几个键，其余原样保留（同 Claude Code 的做法）：
///   · `modelProviders.openai[]` —— 我们那条 provider（按 id 覆盖，不重复追加）
///   · `env.<UKING_ENV_KEY>`     —— Key 的实际值。**settings.json 自带的 env 块就是凭据来源**，
///     所以不必往用户 shell 里塞环境变量（那种做法换个终端就失效）
///   · `security.auth.selectedType = "openai"` —— 不设它，qwen 首次启动仍会弹交互式 /auth
///   · `model.name` —— 默认模型
fn apply_qwen(p: &ProviderPreset, key: &str, model: &str) -> Result<(), String> {
    if key.trim().is_empty() {
        return Err("API Key 不能为空".into());
    }
    let base = p.openai_base.clone();
    if base.trim().is_empty() {
        return Err(format!("{} 不支持 Qwen Code（缺 OpenAI 兼容端点）", p.name));
    }
    let path = qwen_settings_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建 Qwen 配置目录失败: {e}"))?;
    }
    backup_once(&path);

    let mut root: Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| Value::Object(serde_json::Map::new()));
    if !root.is_object() {
        root = Value::Object(serde_json::Map::new());
    }

    let entry = serde_json::json!({
        "id": model,
        "name": format!("{}（U-King）", p.name),
        "baseUrl": base,
        "description": format!("由 U-King 配置：{}", p.name),
        "envKey": UKING_ENV_KEY,
    });
    // modelProviders.openai 是个数组。先清掉 U-King 旧托管项，再放当前真实供应商；
    // 否则换了模型 id 就匹配不到旧项，切十次会积十条。
    {
        let obj = root.as_object_mut().unwrap();
        let mps = obj
            .entry("modelProviders")
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        if !mps.is_object() {
            *mps = Value::Object(serde_json::Map::new());
        }
        let arr = mps
            .as_object_mut()
            .unwrap()
            .entry("openai")
            .or_insert_with(|| Value::Array(vec![]));
        if !arr.is_array() {
            *arr = Value::Array(vec![]);
        }
        let list = arr.as_array_mut().unwrap();
        list.retain(|e| {
            !matches!(
                e.get("envKey").and_then(|v| v.as_str()),
                Some(UKING_ENV_KEY) | Some(LEGACY_UKING_ENV_KEY)
            )
        });
        list.push(entry);
    }
    if let Some(env) = root.get_mut("env").and_then(|e| e.as_object_mut()) {
        env.remove(LEGACY_UKING_ENV_KEY);
    }
    set_json_path(&mut root, &["env", UKING_ENV_KEY], Value::String(key.to_string()));
    set_json_path(
        &mut root,
        &["security", "auth", "selectedType"],
        Value::String("openai".into()),
    );
    set_json_path(&mut root, &["model", "name"], Value::String(model.to_string()));

    let text = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
    atomic_write(&path, text.as_bytes()).map_err(|e| format!("写 Qwen settings.json 失败: {e}"))
}

/// 把驱动写进 Crush 的 `~/.config/crush/crush.json`。
///
/// 🔴 **`providers` 和 `models` 必须一起写。** 只写 `providers` 不写 `models.large/small`，
/// Crush 会无视你配的端点，拿 `OPENAI_API_KEY` 去打 api.openai.com，然后报
/// 「Incorrect API key provided: sk-44fe9***」—— 报错长得就像我们的 Key 坏了，
/// 实际上请求根本没到过虾盘云。本机第一次配就踩到，别让客户再踩一遍。
fn apply_crush(p: &ProviderPreset, key: &str, model: &str) -> Result<(), String> {
    if key.trim().is_empty() {
        return Err("API Key 不能为空".into());
    }
    let base = p.openai_base.clone();
    if base.trim().is_empty() {
        return Err(format!("{} 不支持 Crush（缺 OpenAI 兼容端点）", p.name));
    }
    let path = crush_config_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建 Crush 配置目录失败: {e}"))?;
    }
    backup_once(&path);

    let mut root: Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| Value::Object(serde_json::Map::new()));
    if !root.is_object() {
        root = Value::Object(serde_json::Map::new());
    }

    let provider_id = managed_provider_id(p);
    if let Some(ps) = root.get_mut("providers").and_then(Value::as_object_mut) {
        ps.retain(|id, _| !is_managed_provider_id(id));
    }

    set_json_path(
        &mut root,
        &["providers", &provider_id],
        serde_json::json!({
            "name": format!("{}（U-King）", p.name),
            "type": "openai",
            "base_url": base,
            "api_key": key,
            "models": [{
                "id": model,
                "name": model,
                "context_window": 131072,
                "default_max_tokens": 8192,
            }],
        }),
    );
    // large / small 都指过去。small 不指的话，Crush 拿它跑标题生成那类小任务时
    // 仍会回落到 openai，客户会看到「明明配好了还报 OpenAI 的错」。
    for slot in ["large", "small"] {
        set_json_path(
            &mut root,
            &["models", slot],
            serde_json::json!({ "model": model, "provider": provider_id }),
        );
    }

    let text = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
    atomic_write(&path, text.as_bytes()).map_err(|e| format!("写 crush.json 失败: {e}"))
}

/// OpenCode 全局配置：`~/.config/opencode/opencode.json`
/// （`opencode debug paths` 实测，Windows 上也是这个路径）。
fn opencode_config_path() -> PathBuf {
    config_home().join(".config").join("opencode").join("opencode.json")
}

/// 把驱动写进 OpenCode 的 `~/.config/opencode/opencode.json`。
///
/// 🔴 **OpenCode 只当交互式 TUI 用，不进竞技场。** 2026-08-03 本机三轮实测，
/// `opencode run`（它的非交互入口）**恒定挂满超时、stdout 零字节、stderr 零字节**：
///   · 1.4.3 干净沙箱 HOME + 我们的虾盘云 provider  → 挂 90s
///   · 升到 1.18.11（当时 npm 最新）同样条件      → 挂 100s
///   · **真实 HOME + 用户自己早就配好且缓存过的 provider + `-m z/glm-5`** → 仍挂 100s
///   · 去掉 `--format json` 裸跑 → 挂 45s
/// 第三条是决定性的：既不是我们的配置、也不是沙箱、也不是版本 —— 它的 run 子命令
/// 在这台 Windows 上就是起不来。所以本条目**只保证 TUI 能用**，
/// `apps.ts` 里它也没有非交互提示词。竞技场（系统计时/解析 stdout）**不能带它**，
/// 带了就是拿一个必挂的选手去凑数。
///
/// provider 结构是 OpenCode 自己的形状：`npm: "@ai-sdk/openai-compatible"` + baseURL/apiKey，
/// 已用 `opencode debug config` 验过能被解析（resolved model / providers 都对）。
fn apply_opencode(p: &ProviderPreset, key: &str, model: &str) -> Result<(), String> {
    if key.trim().is_empty() {
        return Err("API Key 不能为空".into());
    }
    let base = p.openai_base.clone();
    if base.trim().is_empty() {
        return Err(format!("{} 不支持 OpenCode（缺 OpenAI 兼容端点）", p.name));
    }
    let path = opencode_config_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建 OpenCode 配置目录失败: {e}"))?;
    }
    backup_once(&path);

    let mut root: Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| Value::Object(serde_json::Map::new()));
    if !root.is_object() {
        root = Value::Object(serde_json::Map::new());
    }
    let provider_id = managed_provider_id(p);
    if let Some(ps) = root.get_mut("provider").and_then(Value::as_object_mut) {
        ps.retain(|id, _| !is_managed_provider_id(id));
    }
    set_json_path(
        &mut root,
        &["provider", &provider_id],
        serde_json::json!({
            "name": format!("{}（U-King）", p.name),
            "npm": "@ai-sdk/openai-compatible",
            "models": { model: { "name": model } },
            "options": { "apiKey": key, "baseURL": base },
        }),
    );
    // 顶层 model 指过去，否则它继续用老的默认模型（实测：不改这个键，
    // 配好 provider 也还是报「Model not found: zhipuai-coding-plan/glm-4.6」）。
    set_json_path(
        &mut root,
        &["model"],
        Value::String(format!("{provider_id}/{model}")),
    );

    let text = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
    atomic_write(&path, text.as_bytes()).map_err(|e| format!("写 opencode.json 失败: {e}"))?;

    // 🔴 **同目录的 `opencode.jsonc` 会压过 `opencode.json` 的顶层 `model`。**
    // 只写 json 就等于「写了个不生效的字段」——GUI 报「已切到 X」，opencode 照跑 jsonc 里那个。
    // 跟 pi 的 defaultProvider 是同一种病：**报告是对的，世界是坏的**。
    //
    // 2026-08-24 沙箱实测（`XDG_CONFIG_HOME` 指到临时目录，两份文件各写一个 provider）：
    //   json  顶层 model = "aaa/from-json"
    //   jsonc 顶层 model = "bbb/from-jsonc"
    //   → `opencode debug config` 解析出 `"model": "bbb/from-jsonc"`，provider 两个都在。
    // 即 **provider 合并、model 由 jsonc 说了算**。用户机器上恰好两份都有（jsonc 里钉着
    // `openrouter/stealth/ox-alpha`），所以在这类机器上我们这次切换必输。
    reconcile_opencode_jsonc_model(&path, &format!("{provider_id}/{model}"))
}

/// 把 `opencode.jsonc` 里那个会压过我们的顶层 `model` 一并对齐。
///
/// 只在「文件存在 **且** 真的钉了顶层 model」时才动它 —— 没这个键就没有冲突，不碰用户文件。
///
/// 🔴 **解析不动就不许瞎写。** `.jsonc` 顾名思义可以带注释和尾逗号，而我们只有严格 JSON
/// 解析器；强行 `to_string_pretty` 回写会把用户的注释全部抹掉（宪法 10：绝不静默覆盖
/// 你没创建的东西）。所以：能严格解析 → 备份后改；解析不了 → **返回错误，把冲突文件
/// 的路径告诉用户**。宁可报「没切成，是这个文件在压着」，也不要报成功后让人自己去猜。
fn reconcile_opencode_jsonc_model(json_path: &std::path::Path, want_model: &str) -> Result<(), String> {
    let jsonc = json_path.with_extension("jsonc");
    let Ok(text) = std::fs::read_to_string(&jsonc) else {
        return Ok(()); // 没有 jsonc = 没有冲突
    };
    let Ok(mut root) = serde_json::from_str::<Value>(&text) else {
        // 带注释/尾逗号，解析不了。不猜、不覆盖，如实报冲突。
        return Err(format!(
            "OpenCode 的 {} 里带注释，我们不敢改它；而它会压过 opencode.json 的模型设置。\
             请手动把里面的 \"model\" 改成 \"{}\"，或先删掉那一行。",
            jsonc.display(),
            want_model
        ));
    };
    // 没钉 model → 不冲突，原样不动（provider 那半边 json 里已经写好、会被合并）。
    if root.get("model").is_none() {
        return Ok(());
    }
    if root.get("model").and_then(|v| v.as_str()) == Some(want_model) {
        return Ok(()); // 已经是我们要的，别做无谓的写入
    }
    backup_once(&jsonc);
    if let Some(o) = root.as_object_mut() {
        o.insert("model".into(), Value::String(want_model.to_string()));
    }
    let out = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
    atomic_write(&jsonc, out.as_bytes()).map_err(|e| format!("写 opencode.jsonc 失败: {e}"))
}

/// pi 的自定义模型表：`~/.pi/agent/models.json`；默认模型在同目录的 `settings.json`。
///
/// 路径和形状都是**从包自带的 `docs/models.md` / `docs/settings.md` 抄的，不是按惯例猜的**
/// （2026-08-03 实测 0.83.0）。Windows 上也是 `~/.pi/`，没有 `%APPDATA%` 变体。
fn pi_models_path() -> PathBuf {
    config_home().join(".pi").join("agent").join("models.json")
}

fn pi_settings_path() -> PathBuf {
    config_home().join(".pi").join("agent").join("settings.json")
}

/// 把驱动写进 pi。**两个文件都要写**：
///   · `models.json` —— provider 定义（baseUrl / api / apiKey / models[]）
///   · `settings.json` 的 `defaultProvider` + `defaultModel` —— 不写它们，pi 的默认 provider
///     是 google，客户装完直接敲 `pi` 会撞上交互式登录，跟「一键配好」的承诺对不上
///
/// 🔴 **`defaultProvider` 和 `defaultModel` 必须一起写，而且 `defaultModel` 是裸 id。**
/// 这里原来只写了 `defaultModel = "<provider>/<model>"`（那是命令行 `--model` 的语法，
/// 不是 settings 的 schema），并且**从不碰 `defaultProvider`** —— 于是客户机上残留的
/// `defaultProvider: "openrouter"` 一直说了算：GUI 报「已切到 DeepSeek」，敲 `pi` 起来
/// 跑的却是 openrouter 的 kimi。**报告是对的，世界是坏的**（2026-08-24 客户实锤）。
///
/// pi 0.83.0 `docs/settings.md` 第 18~19 行写明：`defaultProvider` = provider 名、
/// `defaultModel` = **model ID**（裸的）。本机 `pi -p "hi" --mode json` 四组变异实测：
///   · prov=uking-xiapan + model="uking-xiapan/deepseek-v4-flash" → 实跑 openrouter/kimi ✗
///   · prov=uking-xiapan + model="deepseek-v4-flash"              → 实跑 uking-xiapan/deepseek ✓
///   · prov=openrouter   + model="deepseek-v4-flash"              → 实跑 openrouter/kimi ✗
///   · 无 prov           + model="deepseek-v4-flash"              → 实跑 openrouter/kimi ✗
/// 即：**`defaultProvider` 才是决定权，`defaultModel` 只在它选中的 provider 内匹配。**
fn apply_pi(p: &ProviderPreset, key: &str, model: &str) -> Result<(), String> {
    if key.trim().is_empty() {
        return Err("API Key 不能为空".into());
    }
    // 🔴 trim：自定义供应商的 baseUrl 是客户粘贴进来的，前导/尾随空格一路裸奔到配置文件。
    // 客户机上实测写进去的是 `" https://opencode.ai/zen/go/v1"`（带前导空格）。
    let base = p.openai_base.trim().to_string();
    if base.is_empty() {
        return Err(format!("{} 不支持 pi（缺 OpenAI 兼容端点）", p.name));
    }
    let key = key.trim();
    let model = model.trim();

    let path = pi_models_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建 pi 配置目录失败: {e}"))?;
    }
    backup_once(&path);
    let mut root: Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| Value::Object(serde_json::Map::new()));
    if !root.is_object() {
        root = Value::Object(serde_json::Map::new());
    }
    let provider_id = managed_provider_id(p);
    if let Some(ps) = root.get_mut("providers").and_then(Value::as_object_mut) {
        ps.retain(|id, _| !is_managed_provider_id(id));
    }
    set_json_path(
        &mut root,
        &["providers", &provider_id],
        serde_json::json!({
            // 不再套一层「（U-King）」：预设名本身就带「（U-King 内置）」，
            // 套完客户在 pi 的 /model 里看到的是「虾盘云（U-King 内置）（U-King）」。
            // crush / opencode 那两条是老代码的既成事实，不在这轮一起动。
            "name": p.name.clone(),
            "baseUrl": base,
            "api": "openai-completions",
            "apiKey": key,
            // 🔴 防线，别删：pi 对**声明了 `reasoning: true`** 的模型会把系统提示词发成
            // `developer` 角色，而 new-api（虾盘云）不认这个角色，直接 400
            // `unknown variant `developer``——**第一句话就挂**。
            // 现在下面没写 reasoning，所以不触发；但 deepseek-v4-pro 确确实实是推理模型，
            // 哪天有人顺手补上 `reasoning: true` 就会静默炸掉，而报错信息看着像服务端问题。
            // 这个开关在不触发时**零副作用**（pi 照常发 system 角色），所以无条件写死。
            // 2026-08-04 本机实测：加 reasoning 不加 compat → 400；加了 compat → 正常回话。
            "compat": { "supportsDeveloperRole": false },
            "models": [{
                "id": model,
                "name": model,
                "input": ["text"],
                "contextWindow": 131072,
                "maxTokens": 8192,
            }],
        }),
    );
    let text = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
    atomic_write(&path, text.as_bytes()).map_err(|e| format!("写 pi models.json 失败: {e}"))?;

    // settings.json：两个键一起写，缺一不可（理由见函数头的四组变异实测）。
    let sp = pi_settings_path();
    backup_once(&sp);
    let mut s: Value = std::fs::read_to_string(&sp)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| Value::Object(serde_json::Map::new()));
    if !s.is_object() {
        s = Value::Object(serde_json::Map::new());
    }
    set_json_path(
        &mut s,
        &["defaultProvider"],
        Value::String(provider_id),
    );
    set_json_path(&mut s, &["defaultModel"], Value::String(model.to_string()));
    let text = serde_json::to_string_pretty(&s).map_err(|e| e.to_string())?;
    atomic_write(&sp, text.as_bytes()).map_err(|e| format!("写 pi settings.json 失败: {e}"))
}

fn reset_pi() -> Result<(), String> {
    let path = pi_models_path();
    if !restore_backup(&path) {
        if let Ok(s) = std::fs::read_to_string(&path) {
            if let Ok(mut root) = serde_json::from_str::<Value>(&s) {
                if let Some(ps) = root.get_mut("providers").and_then(|v| v.as_object_mut()) {
                    ps.retain(|id, _| !is_managed_provider_id(id));
                }
                let text = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
                atomic_write(&path, text.as_bytes())
                    .map_err(|e| format!("还原 pi models.json 失败: {e}"))?;
            }
        }
    }
    // defaultModel 指着一个已经删掉的 provider 会让 pi 起手就报错，比留着更糟 —— 一并摘掉。
    let sp = pi_settings_path();
    if restore_backup(&sp) {
        return Ok(());
    }
    let Ok(s) = std::fs::read_to_string(&sp) else {
        return Ok(());
    };
    let Ok(mut root) = serde_json::from_str::<Value>(&s) else {
        return Ok(());
    };
    // 认领判据看 `defaultProvider` —— 现在 `defaultModel` 写的是裸 id，光看它认不出是不是我们写的
    // （客户自己也可能有个同名模型）。老版本写的是 `<provider>/<model>` 形式，一并认掉，
    // 否则从老版本升上来的机器还原之后会剩一条指着已删 provider 的死 defaultModel。
    let ours = root
        .get("defaultProvider")
        .and_then(|v| v.as_str())
        .is_some_and(is_managed_provider_id)
        || root
            .get("defaultModel")
            .and_then(|v| v.as_str())
            .is_some_and(|m| m.split_once('/').is_some_and(|(p, _)| is_managed_provider_id(p)));
    if ours {
        if let Some(o) = root.as_object_mut() {
            o.remove("defaultModel");
            o.remove("defaultProvider");
        }
    }
    let text = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
    atomic_write(&sp, text.as_bytes()).map_err(|e| format!("还原 pi settings.json 失败: {e}"))
}

fn reset_opencode() -> Result<(), String> {
    let path = opencode_config_path();
    // 我们可能改过同目录的 `opencode.jsonc`（它压过 json 的顶层 model，见
    // `reconcile_opencode_jsonc_model`）—— 有备份就先把它还回去，否则用户「还原官方」
    // 之后 opencode 仍然指着我们的 provider，而那个 provider 马上就要被删掉。
    let jsonc = path.with_extension("jsonc");
    if !restore_backup(&jsonc) {
        if let Ok(t) = std::fs::read_to_string(&jsonc) {
            if let Ok(mut r) = serde_json::from_str::<Value>(&t) {
                let ours = r
                    .get("model")
                    .and_then(|v| v.as_str())
                    .is_some_and(|m| m.split_once('/').is_some_and(|(p, _)| is_managed_provider_id(p)));
                if ours {
                    if let Some(o) = r.as_object_mut() {
                        o.remove("model");
                    }
                    let out = serde_json::to_string_pretty(&r).map_err(|e| e.to_string())?;
                    atomic_write(&jsonc, out.as_bytes())
                        .map_err(|e| format!("还原 opencode.jsonc 失败: {e}"))?;
                }
            }
        }
    }
    if restore_backup(&path) {
        return Ok(());
    }
    let Ok(s) = std::fs::read_to_string(&path) else {
        return Ok(());
    };
    let Ok(mut root) = serde_json::from_str::<Value>(&s) else {
        return Ok(());
    };
    if let Some(ps) = root.get_mut("provider").and_then(|v| v.as_object_mut()) {
        ps.retain(|id, _| !is_managed_provider_id(id));
    }
    // 顶层 model 若指着刚删掉的 provider，留着它 OpenCode 会启动即报错 —— 一并摘掉。
    let dangling = root
        .get("model")
        .and_then(|v| v.as_str())
        .map(|m| m.split_once('/').is_some_and(|(p, _)| is_managed_provider_id(p)))
        .unwrap_or(false);
    if dangling {
        if let Some(o) = root.as_object_mut() {
            o.remove("model");
        }
    }
    let text = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
    atomic_write(&path, text.as_bytes()).map_err(|e| format!("还原 OpenCode 配置失败: {e}"))
}

// ============================================================
// Cline CLI（2026-08-29 上架，见 LIST_TOOLS / apply_cline 注释）
// ============================================================

/// Cline 的 provider 配置：`~/.cline/data/settings/providers.json`。
/// 形状是 2026-08-29 让 CLI 自己写一份（`cline auth openai-compatible -k -m -b`）
/// 再回读抄的**权威 schema**，不是猜的：`providers.<id>.settings.{provider,apiKey,model,baseUrl}`。
fn cline_providers_path() -> PathBuf {
    config_home().join(".cline").join("data").join("settings").join("providers.json")
}

/// 🔴 Cline 的 provider id **只认内置 id 表**（实测：`providers.json` 里写自定义 id
/// `uking-xiapan`、类型 `custom` 都被拒「Unknown or disabled provider」）。
/// 所以必须占用通用的 `openai-compatible` 槽位 —— 它是官方 CLI `cline auth` 自己
/// 会写的 id，我们自己实测这条槽位 + 虾盘云端点真回话（6.6s）。
const CLINE_PROVIDER_KEY: &str = "openai-compatible";

/// UTC RFC3339（Cline 的 `updatedAt` 字段格式）。项目没有 chrono（体积优先），
/// 沿用 origin.rs `now_iso` 的 civil_from_days 算法自己算。
fn cline_utc_now() -> String {
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
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.000Z",
        y,
        m,
        d,
        tod / 3600,
        (tod % 3600) / 60,
        tod % 60
    )
}

/// 把驱动写进 Cline。只动 `providers.openai-compatible` 一把 key +
/// `lastUsedProvider` 指针，**绝不整文件重写**（用户的其它 provider 条目、
/// IDE 扩展共享的这份配置一律原样保留）。
fn apply_cline(p: &ProviderPreset, key: &str, model: &str) -> Result<(), String> {
    if key.trim().is_empty() {
        return Err("API Key 不能为空".into());
    }
    let base = p.openai_base.trim().trim_end_matches('/').to_string();
    if base.is_empty() {
        return Err(format!("{} 不支持 Cline（缺 OpenAI 兼容端点）", p.name));
    }
    let path = cline_providers_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建 Cline 配置目录失败: {e}"))?;
    }
    backup_once(&path);
    let mut root: Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({ "version": 1, "modes": {} }));
    if !root.is_object() {
        root = serde_json::json!({ "version": 1, "modes": {} });
    }
    if root.get("modes").is_none() {
        if let Some(o) = root.as_object_mut() {
            o.insert("modes".into(), Value::Object(serde_json::Map::new()));
        }
    }
    set_json_path(
        &mut root,
        &["providers", CLINE_PROVIDER_KEY],
        serde_json::json!({
            "settings": {
                "provider": CLINE_PROVIDER_KEY,
                "apiKey": key,
                "model": model,
                "baseUrl": base,
            },
            "updatedAt": cline_utc_now(),
            "tokenSource": "manual",
        }),
    );
    set_json_path(&mut root, &["lastUsedProvider"], Value::String(CLINE_PROVIDER_KEY.into()));
    let text = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
    atomic_write(&path, text.as_bytes()).map_err(|e| format!("写 Cline 配置失败: {e}"))
}

/// 还原 Cline：优先回滚首次改动前的备份；没有备份就只删我们的
/// `openai-compatible` 槽位 + 摘掉指向它的 lastUsedProvider（用户的其它条目不动）。
fn reset_cline() -> Result<(), String> {
    let path = cline_providers_path();
    if restore_backup(&path) {
        return Ok(());
    }
    let Ok(s) = std::fs::read_to_string(&path) else {
        return Ok(()); // 没配过 = 已经是官方状态
    };
    let Ok(mut root) = serde_json::from_str::<Value>(&s) else {
        return Ok(());
    };
    let mut changed = false;
    if let Some(ps) = root.get_mut("providers").and_then(|v| v.as_object_mut()) {
        if ps.remove(CLINE_PROVIDER_KEY).is_some() {
            changed = true;
        }
    }
    if root.get("lastUsedProvider").and_then(|v| v.as_str()) == Some(CLINE_PROVIDER_KEY) {
        if let Some(o) = root.as_object_mut() {
            o.remove("lastUsedProvider");
        }
        changed = true;
    }
    if !changed {
        return Ok(());
    }
    let text = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
    atomic_write(&path, text.as_bytes()).map_err(|e| format!("还原 Cline 配置失败: {e}"))
}

#[cfg(test)]
mod cline_provider_tests {
    use super::*;

    /// 每个用例一个独立沙箱（`UKING_TEST_HOME`），绝不碰真实的 ~/.cline。
    /// 闭包拿到的是沙箱里的 `.cline/data/settings` 目录。
    fn with_sandbox(tag: &str, f: impl FnOnce(&std::path::Path)) {
        crate::testsandbox::with_sandbox(&format!("cline-{tag}"), &[".cline"], |root| {
            f(&root.join(".cline").join("data").join("settings"))
        })
    }

    fn preset() -> ProviderPreset {
        ProviderPreset {
            id: "xiapan".into(),
            name: "虾盘云（U-King 内置）".into(),
            summary: String::new(),
            // 故意带尾斜杠：apply 必须 trim（Cline 对 baseUrl 敏感）
            openai_base: "https://api.u-claw.org.cn/v1/".into(),
            anthropic_base: None,
            model: "deepseek-v4-flash".into(),
            small_model: "deepseek-v4-flash".into(),
            codex_model: String::new(),
            codex_wire_api: WIRE_API.into(),
            key_url: String::new(),
            key_hint: String::new(),
            builtin_recharge: true,
            recommended: true,
            builtin: true,
            api_key: String::new(),
        }
    }

    /// 用户自有配置（CLI 写出的形状 + 用户自己配过的条目）。
    const USER_PROVIDERS_JSON: &str = r#"{
  "version": 1,
  "modes": {},
  "providers": {
    "anthropic": {
      "settings": { "provider": "anthropic", "apiKey": "sk-ant-user", "model": "claude-x" },
      "updatedAt": "2026-08-01T00:00:00.000Z",
      "tokenSource": "manual"
    }
  },
  "lastUsedProvider": "anthropic"
}"#;

    /// 🔴 2026-08-30 发版会审条件（opus）：apply 只动 `openai-compatible` 槽位 +
    /// `lastUsedProvider` 指针，用户自有条目逐字段不变；baseUrl 尾斜杠 trim。
    #[test]
    fn apply_only_touches_our_slot() {
        with_sandbox("apply-slot", |dir| {
            std::fs::create_dir_all(dir).unwrap();
            let path = dir.join("providers.json");
            std::fs::write(&path, USER_PROVIDERS_JSON).unwrap();

            apply_cline(&preset(), "sk-xp-test", "deepseek-v4-flash").unwrap();

            let after: Value =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            let orig: Value = serde_json::from_str(USER_PROVIDERS_JSON).unwrap();
            assert_eq!(
                after["providers"]["anthropic"], orig["providers"]["anthropic"],
                "用户自有条目被动了"
            );
            let slot = &after["providers"]["openai-compatible"]["settings"];
            assert_eq!(slot["apiKey"], "sk-xp-test");
            assert_eq!(slot["model"], "deepseek-v4-flash");
            assert_eq!(slot["baseUrl"], "https://api.u-claw.org.cn/v1", "尾斜杠必须 trim");
            assert_eq!(after["lastUsedProvider"], "openai-compatible");
        });
    }

    /// apply 前已有文件 → backup_once 留锚点 → reset 走备份回滚，整文件回到改前。
    #[test]
    fn reset_restores_backup_after_apply() {
        with_sandbox("reset-bak", |dir| {
            std::fs::create_dir_all(dir).unwrap();
            let path = dir.join("providers.json");
            std::fs::write(&path, USER_PROVIDERS_JSON).unwrap();

            apply_cline(&preset(), "sk-xp-test", "deepseek-v4-flash").unwrap();
            reset_cline().unwrap();

            let after: Value =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            let orig: Value = serde_json::from_str(USER_PROVIDERS_JSON).unwrap();
            assert_eq!(after, orig, "备份回滚后必须与改前逐字段一致");
            assert!(after["providers"].get("openai-compatible").is_none());
        });
    }

    /// 没有备份（客户手工删过 .uking-bak 的形状）：reset 只摘我们的槽位与指针，用户条目不动。
    #[test]
    fn reset_without_backup_keeps_user_entries() {
        with_sandbox("reset-nobak", |dir| {
            std::fs::create_dir_all(dir).unwrap();
            let path = dir.join("providers.json");
            let taken_over = r#"{
  "version": 1,
  "modes": {},
  "providers": {
    "anthropic": { "settings": { "provider": "anthropic", "apiKey": "sk-ant-user", "model": "claude-x" } },
    "openai-compatible": { "settings": { "provider": "openai-compatible", "apiKey": "sk-xp-old", "model": "old", "baseUrl": "https://api.u-claw.org.cn/v1" } }
  },
  "lastUsedProvider": "openai-compatible"
}"#;
            std::fs::write(&path, taken_over).unwrap();
            reset_cline().unwrap();
            let after: Value =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            assert!(after["providers"].get("openai-compatible").is_none(), "我们的槽位要摘掉");
            assert_eq!(
                after["providers"]["anthropic"]["settings"]["apiKey"], "sk-ant-user",
                "用户条目不能动"
            );
            assert!(after.get("lastUsedProvider").is_none(), "指向我们的指针要摘掉");
        });
    }

    /// 从零 apply（客户没配过 Cline）：造出合法形状，modes 键补齐。
    #[test]
    fn apply_creates_shape_from_scratch() {
        with_sandbox("fresh", |dir| {
            std::fs::create_dir_all(dir).unwrap();
            apply_cline(&preset(), "sk-xp-test", "deepseek-v4-flash").unwrap();
            let path = dir.join("providers.json");
            let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            assert_eq!(v["lastUsedProvider"], "openai-compatible");
            assert!(v["providers"]["openai-compatible"]["settings"]["baseUrl"].is_string());
            assert!(v.get("modes").is_some(), "Cline 的 modes 键必须存在");
        });
    }

    /// 空 Key 必须被拒（防呆：客户没填 Key 就点应用）。
    #[test]
    fn apply_rejects_empty_key() {
        with_sandbox("empty-key", |_| {
            let err = apply_cline(&preset(), "   ", "m").unwrap_err();
            assert!(err.contains("Key"), "应报 Key 为空，实际: {err}");
        });
    }

    /// 缺 OpenAI 端点的预设必须明确报「不支持」，而不是把空 baseUrl 写进配置。
    #[test]
    fn apply_rejects_missing_openai_base() {
        with_sandbox("no-openai", |dir| {
            std::fs::create_dir_all(dir).unwrap();
            let mut p = preset();
            p.openai_base = String::new();
            let err = apply_cline(&p, "sk-xp-test", "m").unwrap_err();
            assert!(err.contains("不支持"), "应报不支持，实际: {err}");
            assert!(!dir.join("providers.json").exists(), "失败时不应写出文件");
        });
    }
}

/// 还原官方：优先回滚首次改动前留的备份；没有备份就只摘掉我们写进去的那部分，
/// **绝不整文件删** —— 用户可能自己在同一个文件里配了别的 provider。
fn reset_qwen() -> Result<(), String> {
    let path = qwen_settings_path();
    if restore_backup(&path) {
        return Ok(());
    }
    let Ok(s) = std::fs::read_to_string(&path) else {
        return Ok(()); // 没配过 = 已经是官方状态
    };
    let Ok(mut root) = serde_json::from_str::<Value>(&s) else {
        return Ok(());
    };
    if let Some(list) = root
        .get_mut("modelProviders")
        .and_then(|m| m.get_mut("openai"))
        .and_then(|a| a.as_array_mut())
    {
        list.retain(|e| {
            !matches!(
                e.get("envKey").and_then(|v| v.as_str()),
                Some(UKING_ENV_KEY) | Some(LEGACY_UKING_ENV_KEY)
            )
        });
    }
    if let Some(env) = root.get_mut("env").and_then(|e| e.as_object_mut()) {
        env.remove(UKING_ENV_KEY);
        env.remove(LEGACY_UKING_ENV_KEY);
    }
    let text = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
    atomic_write(&path, text.as_bytes()).map_err(|e| format!("还原 Qwen 配置失败: {e}"))
}

fn reset_crush() -> Result<(), String> {
    // 存量客户机上还躺着一份老版本写错位置的配置（~/.config/crush）。它没人读，
    // 但「还原」说了要清就得真清干净，否则足迹对不上账。best-effort，失败不影响主路径。
    #[cfg(windows)]
    {
        let legacy = crush_legacy_config_path();
        if legacy.exists() && !restore_backup(&legacy) {
            let _ = std::fs::remove_file(&legacy);
        }
    }
    let path = crush_config_path();
    if restore_backup(&path) {
        return Ok(());
    }
    let Ok(s) = std::fs::read_to_string(&path) else {
        return Ok(());
    };
    let Ok(mut root) = serde_json::from_str::<Value>(&s) else {
        return Ok(());
    };
    if let Some(ps) = root.get_mut("providers").and_then(|v| v.as_object_mut()) {
        ps.retain(|id, _| !is_managed_provider_id(id));
    }
    // models 指着一个已删掉的 provider 会让 Crush 起不来，比留着更糟 —— 一并摘掉。
    if let Some(ms) = root.get_mut("models").and_then(|v| v.as_object_mut()) {
        for slot in ["large", "small"] {
            let ours = ms
                .get(slot)
                .and_then(|v| v.get("provider"))
                .and_then(|v| v.as_str())
                .is_some_and(is_managed_provider_id);
            if ours {
                ms.remove(slot);
            }
        }
    }
    let text = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
    atomic_write(&path, text.as_bytes()).map_err(|e| format!("还原 Crush 配置失败: {e}"))
}

/// 在 JSON 里按路径写值，中间缺的对象自动补出来（`root["a"]["b"] = v`）。
fn set_json_path(root: &mut Value, path: &[&str], value: Value) {
    let mut cur = root;
    for (i, seg) in path.iter().enumerate() {
        if !cur.is_object() {
            *cur = Value::Object(serde_json::Map::new());
        }
        let obj = cur.as_object_mut().unwrap();
        if i == path.len() - 1 {
            obj.insert((*seg).to_string(), value);
            return;
        }
        cur = obj
            .entry((*seg).to_string())
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
    }
}

/// 有 `*.uking-bak` 就拷回来（返回 true = 已还原）。
fn restore_backup(f: &std::path::Path) -> bool {
    let bak = f.with_extension(format!(
        "{}.uking-bak",
        f.extension().and_then(|e| e.to_str()).unwrap_or("cfg")
    ));
    bak.exists() && std::fs::copy(&bak, f).is_ok()
}

/// ClawX 4.x 真实配置文件（schemaVersion 2）。ClawX 是 Electron 应用，配置落在各平台的
/// Electron `userData` 目录：
///  - Windows: `%APPDATA%\ClawX`（= `~/AppData/Roaming/ClawX`）
///  - macOS:   `~/Library/Application Support/ClawX`
///  - Linux:   `~/.config/ClawX`（兜底）
///
/// 🔴 macOS 修复（客户 MacBook 实锤，2026-07-19）：老代码无条件读 `APPDATA` ——
/// Mac 上根本没这个环境变量 → `appdata=""` → 路径塌成相对 `ClawX/clawx-providers.json`，
/// Finder 启动时 CWD=`/` 不可写 → `create_dir_all` 报错 → **「一键配置 ClawX 失败」**。
/// 必须按平台分支取 Electron userData 目录。
///
/// ⚠️ ClawX 4.x **不再读** `~/.openclaw/openclaw.json` 的 models 节点 —— 它有自己的
/// provider 存储（providerAccounts + apiKeys + providerSecrets + defaultProvider）。
/// 老代码写 openclaw.json 对 ClawX 4.x 完全无效（实测：切了 ClawX 没反应的真因）。
/// 支持 UKING_TEST_HOME 沙箱（重定向到 `<沙箱>/ClawX/clawx-providers.json`）。
fn clawx_providers_path() -> PathBuf {
    if let Ok(t) = std::env::var("UKING_TEST_HOME") {
        if !t.trim().is_empty() {
            return PathBuf::from(t).join("ClawX").join("clawx-providers.json");
        }
    }
    #[cfg(windows)]
    let dir = {
        let appdata = std::env::var("APPDATA").unwrap_or_default();
        PathBuf::from(appdata).join("ClawX")
    };
    #[cfg(target_os = "macos")]
    let dir = {
        let home = std::env::var("HOME").unwrap_or_default();
        PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("ClawX")
    };
    #[cfg(not(any(windows, target_os = "macos")))]
    let dir = {
        let home = std::env::var("HOME").unwrap_or_default();
        PathBuf::from(home).join(".config").join("ClawX")
    };
    dir.join("clawx-providers.json")
}

/// 当前 UTC 时间的 ISO-8601 毫秒字符串（如 `2026-06-24T10:30:00.000Z`），无第三方依赖。
/// ClawX 的 provider 账号要求 `createdAt`/`updatedAt` 为该格式（等价 JS `new Date().toISOString()`）。
/// 漏写会让 ClawX 前端按 `updatedAt.localeCompare(...)` 排序时对 `undefined` 取属性而崩溃
/// （整页 "Something went wrong"）。用 SystemTime + civil_from_days 手算，契合本文件"不引额外依赖"的风格。
fn now_iso8601() -> String {
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs() as i64;
    let millis = dur.subsec_millis();
    let days = secs.div_euclid(86_400);
    let tod = secs.rem_euclid(86_400);
    let (hh, mm, ss) = (tod / 3600, (tod % 3600) / 60, tod % 60);
    // Howard Hinnant civil_from_days：天数 → (年, 月, 日)
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        y, m, d, hh, mm, ss, millis
    )
}

/// 接管 ClawX 4.x 模型：往 `clawx-providers.json` 写一个如实命名的 `uking-*`
/// 账号（baseUrl+model），
/// 把 key 写进 `apiKeys` + `providerSecrets`，并设为 `defaultProvider`。
/// 只动我们这个账号 + 默认指向，别人的账号保留；首次改动前备份。
///
/// ⚠️ ClawX 运行时持有内存副本，写完文件 **需重启 ClawX 才生效**（前端 toast 已提示）。
fn apply_clawx(p: &ProviderPreset, key: &str, model: &str) -> Result<(), String> {
    if key.trim().is_empty() {
        return Err("API Key 不能为空".into());
    }
    let base = p.openai_base.clone();
    if base.trim().is_empty() {
        return Err(format!("{} 不支持 ClawX（缺 OpenAI 兼容端点）", p.name));
    }
    let path = clawx_providers_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建 ClawX 配置目录失败: {e}"))?;
    }
    backup_once(&path);

    // ClawX 账号必须带 createdAt/updatedAt（ISO 字符串），否则前端排序崩，详见 now_iso8601 注释。
    let now = now_iso8601();

    // 读现有配置（不存在则建一个 schemaVersion 2 骨架）
    let mut root: Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({ "schemaVersion": 2 }));
    if !root.is_object() {
        root = json!({ "schemaVersion": 2 });
    }
    let obj = root.as_object_mut().unwrap();
    obj.entry("schemaVersion").or_insert_with(|| json!(2));

    // 账号 id 如实带上真实上游，同时保留 `uking-` 命名空间避免撞客户自己的账号。
    let acct_id = managed_provider_id(p);

    // 先清掉 U-King 历史遗留的脏账号（修「一升级就乱」）：
    // 早期版本曾用过 "uking" / "xiapan" 等裸 id（缺 model、vendorId=custom、isDefault=false），
    // 它们指向虾盘云端点但不被新代码接管 → 残留在列表里干扰回显/选择。
    // 🔴 还有带 custom- 前缀的变体（本机 clawx-providers.json 实测躺着 custom-uking /
    // custom-xiapan 两条）：无 key、model 为 undefined，且 baseUrl 是**空的**——所以
    // 「指向虾盘云」那条判据永远不命中，它们能一直存活。剥掉 custom- 前缀后再对脏名单，
    // 两代遗留一起清。客户自己加的别的 provider（deepseek 等非虾盘云端点）一律保留；
    // 客户自建账号 id 由 ClawX 随机生成（实测形如 custom-custom51），不会撞这两条保留字。
    const STALE_IDS: &[&str] = &["uking", "xiapan"];
    let prune = |accounts: &mut serde_json::Map<String, Value>| {
        let to_del: Vec<String> = accounts
            .iter()
            .filter(|(id, a)| {
                if id.as_str() == acct_id {
                    return false;
                }
                let bare = id.strip_prefix("custom-").unwrap_or(id);
                // 🔴 名字撞上裸保留字**还不够删**：客户手动自建的账号 id 恰好叫
                // custom-uking / custom-xiapan 的中转会被误伤（GPT/pi 双终审同判）。
                // 叠加「历史孤儿特征」佐证 —— 我们的遗留脏号必然没有真实端点
                // （baseUrl 为空或仍指向虾盘云）；有真实第三方端点的同名账号一律放过。
                let no_real_base = a
                    .get("baseUrl")
                    .and_then(|x| x.as_str())
                    .map(|u| u.trim().is_empty() || is_xiapan_endpoint(u))
                    .unwrap_or(true);
                let is_stale_id = STALE_IDS.contains(&bare) && no_real_base;
                let is_old_managed = is_managed_provider_id(id);
                let to_xiapan = a
                    .get("baseUrl")
                    .and_then(|x| x.as_str())
                    .map(|u| is_xiapan_endpoint(u))
                    .unwrap_or(false);
                is_stale_id || is_old_managed || to_xiapan
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in &to_del {
            accounts.remove(id);
        }
        to_del
    };

    let accounts = obj
        .entry("providerAccounts")
        .or_insert_with(|| json!({}));
    let accounts = accounts.as_object_mut().ok_or("providerAccounts 不是对象")?;
    let pruned = prune(accounts);
    // 其它账号 isDefault 置 false（我们要当默认）；
    // 顺手给缺时间戳的历史账号补 createdAt/updatedAt：老版本 U-King 写过不带时间戳的脏账号，
    // 列表里只要有一条这样的，ClawX 的 updatedAt.localeCompare 排序就会崩 → 这里自愈已装机客户。
    for (_id, a) in accounts.iter_mut() {
        if let Some(o) = a.as_object_mut() {
            o.insert("isDefault".into(), json!(false));
            if !o.get("createdAt").map(|v| v.is_string()).unwrap_or(false) {
                o.insert("createdAt".into(), json!(now.clone()));
            }
            if !o.get("updatedAt").map(|v| v.is_string()).unwrap_or(false) {
                o.insert("updatedAt".into(), json!(now.clone()));
            }
        }
    }
    accounts.insert(
        acct_id.clone(),
        json!({
            "id": acct_id,
            "vendorId": "custom",
            "label": p.name,
            "authMode": "api_key",
            "baseUrl": base,
            "model": model,         // 纯模型 id（不带 provider/ 前缀，否则 ClawX 解析错）
            "enabled": true,
            "isDefault": true,
            "createdAt": now.clone(),   // 缺这两个 ClawX 前端排序会崩（localeCompare on undefined）
            "updatedAt": now.clone()
        }),
    );

    // key 写两处（ClawX 读 providerSecrets，apiKeys 是冗余镜像，都写齐避免「没保存 key」）
    // 同时清掉刚才 prune 掉的脏账号在这两处的残留 key。
    let api_keys = obj.entry("apiKeys").or_insert_with(|| json!({}));
    if let Some(o) = api_keys.as_object_mut() {
        for id in &pruned {
            o.remove(id);
        }
        o.insert(acct_id.clone(), json!(key));
    }
    let secrets = obj.entry("providerSecrets").or_insert_with(|| json!({}));
    if let Some(o) = secrets.as_object_mut() {
        for id in &pruned {
            o.remove(id);
        }
        o.insert(
            acct_id.clone(),
            json!({ "type": "api_key", "accountId": acct_id, "apiKey": key }),
        );
    }

    // 默认 provider 指向我们
    obj.insert("defaultProvider".into(), json!(acct_id));
    obj.insert("defaultProviderAccountId".into(), json!(acct_id));

    atomic_write(&path, serde_json::to_string_pretty(&root).unwrap().as_bytes())
        .map_err(|e| format!("写 clawx-providers.json 失败: {e}"))?;

    // 同步 ClawX 内嵌 OpenClaw agent 层（修「agent 报 No API key for provider openai」）：
    // clawx-providers.json 只管 ClawX 的「聊天 provider 选择」，但 ClawX 跑 agent/gateway 时
    // 走的是 ~/.openclaw 的 openclaw.json + agents/<id>/agent/{models,auth-profiles}.json。
    // 这三处不写，agent 找不到 provider 定义就 fallback 到默认 openai → 报缺 key。
    // best-effort：写不动不让整次 ClawX 切换失败（agent 层是附加项）。
    let _ = apply_openclaw_agent(&acct_id, &base, key, model);
    Ok(())
}

/// OpenClaw **agent 层** provider 键 —— **必须等于 ClawX 从账号 id 派生的键**。
///
/// 历史 bug（pc-*** 实证复现）：早期这里写裸键 `"uking"`，但 ClawX 的 `getActiveOpenClawProviders`
/// 会把 openclaw.json 里 `models.providers` 的键当作「活跃 provider」，再用 `resolveOpenClawProviderKey`
/// 把账号 `uking-xiapan` 算成键 `custom-ukingxia`。两者不等 → ClawX 认为多了个没有匹配账号的活跃键，
/// 于是反向 seed 一个**无 key 孤儿**「Uking」到列表里。对齐派生键后就不再冒孤儿。
///
/// 派生公式镜像 ClawX `provider-keys.ts`：`custom-<账号 id 去横线后取前 8 字符>`
/// （`uking-xiapan` → `ukingxiapan` → `ukingxia` → `custom-ukingxia`）。
fn clawx_agent_provider_key(account_id: &str) -> String {
    let compact: String = account_id.chars().filter(|c| *c != '-').take(8).collect();
    format!("custom-{compact}")
}

/// agent 层历史遗留的脏 provider 键（早期写过的裸键 / 被 seed 的孤儿），每次配置时一并清掉，防复发。
const STALE_AGENT_KEYS: &[&str] = &["uking", "xiapan"];

fn is_managed_agent_provider_key(id: &str) -> bool {
    id.starts_with("custom-uking") || STALE_AGENT_KEYS.contains(&id)
}

/// OpenClaw 配置根目录（`~/.openclaw`，支持 UKING_TEST_HOME 沙箱）。
fn openclaw_home() -> PathBuf {
    config_home().join(".openclaw")
}

const OPENCLAW_MODEL_TIMEOUT_SECONDS: u32 = 600;

/// provider 级输出上限（写进 openclaw.json 的 `maxTokens`）。
const OPENCLAW_MAX_TOKENS: u64 = 8192;

/// 压缩预留下限（`agents.defaults.compaction.reserveTokensFloor`）——**必须跟上面的
/// `maxTokens` 对齐**。ClawX 自带默认是 **50000**，那是给「输出上限就有几万」的模型定的；
/// 我们把 maxTokens 钉死 8192 却没管它，等于给一个最多吐 8192 的模型留了 50000 的出口。
///
/// 🔴 pc-***（2026-08-02，远程日志实证）就死在这：可用 prompt 预算 = contextWindow − reserve
/// = 131072 − 50000 = **81072**，131k 的窗口只能用 81k，白冻 32%。日志里
/// `overflowTokens=225` / `264` —— **溢出 225 个 token 就触发一次压缩**，而压缩在低内存机器上
/// 撞 180 秒硬死线失败（`reason=timeout durationMs=180020`），恢复链耗尽后会话被判
/// `livenessState=blocked`，客户看到的就是「上下文崩溃了、只能删了重开」。
///
/// 取 2×maxTokens：留足输出 + 余量，可用预算 131072−16384 = **114688（+41%）**，
/// 让那些「溢出两三百 token」的会话根本不触发压缩。
const OPENCLAW_RESERVE_TOKENS_FLOOR: u64 = OPENCLAW_MAX_TOKENS * 2;

/// 主模型失败后的兜底链（`agents.defaults.model.fallbacks`）——🔴 **不能是空数组**。
///
/// 客户 issue #38（2026-08-22 定性）：旧实现把 fallbacks 写死
/// `[]`，而 ClawX 内置 provider 名下永远没有我们的 Key，于是主模型一报 auth 失败
/// 当场 `chain_exhausted`（OpenClaw model-fallback 日志原文），连「退到 flash 再试一次」
/// 的机会都没有 —— 这是唯一能让**存量客户不重开会话就自愈**的改法：已有会话的模型
/// 是跟会话钉死的，改配置只救新会话，但兜底链是运行时读的。
///
/// 元素格式 = `"provider/model"` 引用字符串（消费方 OpenClaw
/// `resolveAgentModelFallbackValues` 只认数组、逐项 `resolveModelRefFromString`
/// 解析；两个 id 都必须在我们写出的 provider 的 `models` 数组里声明过，否则解析不到）。
///
/// 语义边界（读过引擎源码核实）：主模型仍是我们时兜底链原样生效；客户自己在 ClawX 里
/// 把 primary 切到别家后，引擎发现 fallback 指向的候选与 effectivePrimary 相同就清空它
/// —— 即**尊重客户的路由选择，不会把他拽回来**。
fn openclaw_fallback_chain(account_id: &str, primary_model: &str) -> Vec<String> {
    // ① 只给虾盘云自己的路由写兜底 —— 别家供应商不一定提供 flash，写死就是伪兜底：
    //   主模型挂掉后它会再吃一次「模型不存在」才最终报错（GPT 终审 2026-08-26）。
    // ② 主模型本身就是 flash 时返回空：兜底=主模型 = 原地把同一模型重试一遍，
    //   白烧一个 timeoutSeconds 才报错（pi 终审 2026-08-26）。空数组时引擎行为
    //   退回「失败即报」，与改动前的写死 [] 一致，不会更糟。
    if account_id != XIAPAN_MANAGED_ACCOUNT || primary_model == OPENCLAW_SMALL_FAST_MODEL {
        return Vec::new();
    }
    vec![format!("{XIAPAN_MANAGED_ACCOUNT}/{OPENCLAW_SMALL_FAST_MODEL}")]
}

/// 兜底链用的省 token 小模型。与 Codex 侧同一取舍：主模型用满血，兜底用 flash。
const OPENCLAW_SMALL_FAST_MODEL: &str = "deepseek-v4-flash";

/// 虾盘云在 ClawX 账号层的托管 id（= managed_provider_id(xiapan)；slug 规则下原样保留）。
const XIAPAN_MANAGED_ACCOUNT: &str = "uking-xiapan";

/// 把虾盘云写进 ClawX 内嵌 OpenClaw 的 **agent 运行时配置**（参考 ClawX 官方 openclaw-auth.ts
/// 的 saveProviderKeyToOpenClaw + updateAgentModelProvider + setOpenClawDefaultModelWithOverride
/// 三件套，实现于 U-King 侧）。写三处，缺一不可：
///  ① `~/.openclaw/openclaw.json`：models.providers.uking（provider 定义）+ agents.defaults.model.primary
///     = "uking/<model>"（告诉 agent 默认就用这个，否则 fallback openai）
///  ② `~/.openclaw/agents/<id>/agent/models.json`：providers.uking（带真实 apiKey）
///  ③ `~/.openclaw/agents/<id>/agent/auth-profiles.json`：profiles["uking:default"] + order + lastGood
///
/// base 是 OpenAI 兼容端点（虾盘云 /v1）；api 固定 openai-completions。
fn apply_openclaw_agent(account_id: &str, base: &str, key: &str, model: &str) -> Result<(), String> {
    // 写**两个** home，缺一 openclaw cli 就没配置：
    //  ① `~/.openclaw`         —— ClawX 桌面版内嵌 agent 读这份（GUI 路径）
    //  ② `~/.uking/openclaw`   —— U-King 终端(term.rs)起 openclaw cli 时把 OPENCLAW_HOME 指到这（CLI 路径）
    // 历史 bug（2026-07-20 客户实锤 + 三 home 对比实证）：只写 ①，PTY 里的 openclaw cli 读 ② = 空配置
    // → 打开报「未配置」弹 setup 安全墙。两份都写齐，openclaw cli 才能「打开即对话」（对齐 hermes）。
    for home in [openclaw_home(), config_home().join(".uking").join("openclaw")] {
        apply_openclaw_agent_to_home(&home, account_id, base, key, model)?;
    }
    Ok(())
}

fn apply_openclaw_agent_to_home(
    home: &std::path::Path,
    account_id: &str,
    base: &str,
    key: &str,
    model: &str,
) -> Result<(), String> {
    let prov_owned = clawx_agent_provider_key(account_id);
    let prov = prov_owned.as_str();
    let model_entry = json!({ "id": model, "name": model });
    // 兜底模型也要在 provider 的 models 数组里声明，否则引擎解析 fallback 引用时找不到
    // （主模型恰好就是 flash 时数组去重，不重复声明）。
    let fallback_models: Vec<Value> = openclaw_fallback_chain(prov, model)
        .iter()
        .filter_map(|raw| raw.rsplit('/').next())
        .filter(|m| *m != model)
        .map(|m| json!({ "id": m, "name": m }))
        .collect();
    let mut provider_models = vec![model_entry.clone()];
    provider_models.extend(fallback_models);

    // ① openclaw.json —— provider 定义 + 默认模型
    let oc_path = home.join("openclaw.json");
    if let Some(d) = oc_path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let mut oc: Value = std::fs::read_to_string(&oc_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({}));
    if !oc.is_object() {
        oc = json!({});
    }
    {
        let root = oc.as_object_mut().unwrap();
        // models.providers.uking
        let models = root.entry("models").or_insert_with(|| json!({}));
        let models = models.as_object_mut().ok_or("openclaw.json models 不是对象")?;
        let providers = models.entry("providers").or_insert_with(|| json!({}));
        let providers = providers.as_object_mut().ok_or("models.providers 不是对象")?;
        providers.retain(|id, _| !is_managed_agent_provider_key(id) || id == prov);
        providers.insert(
            prov.into(),
            json!({
                "baseUrl": base,
                "api": "openai-completions",
                "apiKey": key,             // 真实 key（ClawX 内嵌网关直接读这里，写 profile 名会 401）
                "timeoutSeconds": OPENCLAW_MODEL_TIMEOUT_SECONDS,
                // 输出上限（openclaw 文档 model-providers.md：provider 级 maxTokens = 所有模型默认）。
                // 够大才能让推理型模型的 reasoning_content + 正文都放下，否则正文空→「无法生成回复」。
                // 改这个数必须同步看 OPENCLAW_RESERVE_TOKENS_FLOOR（两者不对齐 = pc-*** 那个 bug）。
                "maxTokens": OPENCLAW_MAX_TOKENS,
                // 主模型 + 兜底链引用的模型都要声明（fallback 引用未声明的 id 会被引擎跳过）
                "models": provider_models,
            }),
        );
        // 清掉历史脏键（裸 "uking"/"xiapan"）——否则 ClawX 据此 seed 出无 key 孤儿
        // agents.defaults.model.primary = "custom-ukingxia/<model>"
        let agents = root.entry("agents").or_insert_with(|| json!({}));
        let agents = agents.as_object_mut().ok_or("openclaw.json agents 不是对象")?;
        let defaults = agents.entry("defaults").or_insert_with(|| json!({}));
        let defaults = defaults.as_object_mut().ok_or("agents.defaults 不是对象")?;
        // 🔴 fallbacks 绝不能写空数组：主模型 auth/网络失败时引擎直接 chain_exhausted，
        // 客户看到「AI 没反应」而没有任何自愈机会（issue #38 的根因）。
        defaults.insert(
            "model".into(),
            json!({
                "primary": format!("{prov}/{model}"),
                "fallbacks": openclaw_fallback_chain(prov, model),
            }),
        );
        defaults.insert("timeoutSeconds".into(), json!(OPENCLAW_MODEL_TIMEOUT_SECONDS));

        // 压缩预留：跟 maxTokens 对齐（见 OPENCLAW_RESERVE_TOKENS_FLOOR 的注释 / pc-***）。
        // **只放松，不收紧**：仅当现值缺失或比我们的上限还大时才改小 —— 用户若自己调到更低
        // （更激进地用满上下文）是他的选择，我们不许把它调回来（宪法第 10 条：不碰用户真实状态）。
        // `mode` 一律不动，那是 ClawX 的策略开关不是我们的。
        {
            let compaction = defaults.entry("compaction").or_insert_with(|| json!({}));
            if let Some(c) = compaction.as_object_mut() {
                let too_greedy = c
                    .get("reserveTokensFloor")
                    .and_then(|v| v.as_u64())
                    .map_or(true, |cur| cur > OPENCLAW_RESERVE_TOKENS_FLOOR);
                if too_greedy {
                    c.insert(
                        "reserveTokensFloor".into(),
                        json!(OPENCLAW_RESERVE_TOKENS_FLOOR),
                    );
                }
            }
        }

        // setup 向导「已完成」标记：openclaw cli 首次交互会弹一道英文安全墙（"...requires lock-down.
        // Continue? Yes/No"），本质是 setup 向导首跑门；`wizard.lastRunAt` 一旦有值 openclaw 就当 setup
        // 做完、不再弹。既然模型已由我们配好 = setup 等价完成，预置这个时间戳让小白「打开即对话」
        // （对齐 hermes 体验，2026-07-20 客户实锤"一打开就要配置"的根因）。已有真实值则不覆盖。
        let wizard = root.entry("wizard").or_insert_with(|| json!({}));
        if let Some(w) = wizard.as_object_mut() {
            w.entry("lastRunAt").or_insert_with(|| json!(now_iso8601()));
        }
    }
    backup_once(&oc_path);
    atomic_write(&oc_path, serde_json::to_string_pretty(&oc).unwrap().as_bytes())
        .map_err(|e| format!("写 openclaw.json 失败: {e}"))?;

    let _ = ensure_openclaw_text_commands();

    // ②③ 每个 agent（agents/ 下的子目录，没有就默认 main）
    let agents_dir = home.join("agents");
    let mut agent_ids: Vec<String> = std::fs::read_dir(&agents_dir)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.path().is_dir())
                .filter_map(|e| e.file_name().to_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    if agent_ids.is_empty() {
        agent_ids.push("main".into());
    }
    for id in agent_ids {
        let adir = agents_dir.join(&id).join("agent");
        if std::fs::create_dir_all(&adir).is_err() {
            continue;
        }
        // ② models.json
        write_agent_models_json(&adir.join("models.json"), prov, base, key, &provider_models);
        // ③ auth-profiles.json
        write_agent_auth_profiles(&adir.join("auth-profiles.json"), prov, key);
        // ④ 清掉 agent 的 sqlite auth 缓存（含 wal/shm），逼它下次启动从上面的
        //    auth-profiles.json（portable static auth）重新导入 —— 否则旧的脏 profile
        //    （如裸 "uking"）会残留在 sqlite 里，孤儿照旧出现。
        for suffix in ["", "-wal", "-shm", "-journal"] {
            let _ = std::fs::remove_file(adir.join(format!("openclaw-agent.sqlite{suffix}")));
        }
    }
    Ok(())
}

/// 开 OpenClaw 的 `commands.text`（chat 内 `/skill-name` 斜杠触发技能）。
///
/// 真实客户实锤（2026-07-08）：装好 uking-aigc 技能后在 ClawX 里打 `/uking-aigc <需求>`，
/// agent 把它当成一串从没见过定义的陌生斜杠文本，反复怀疑是提示注入、拒绝执行、循环追问确认——
/// 不是技能装坏了。根因：OpenClaw 官方本就支持 `/<skill-name>` 确定性派发（不经模型判断就把消息体
/// 改写成 `Use the "<skill>" skill for this request...`），但要靠 `commands.text` 这个开关打开
/// （schema 原话：默认給"advanced"标签，help 里写"Keep this enabled for compatibility..."暗示期望
/// 常开，但实测客户机 openclaw.json 里只有 `commands.restart`，没有 `commands.text`，说明没人替
/// 客户开过）。开这个只影响聊天框斜杠解析，不碰 models/agents，副作用面小。
///
/// 只在缺失时补 `true`，不覆盖用户/其它工具已写的显式值（哪怕是 false）——尊重既有选择。
/// `.openclaw` 目录不存在（没装 OpenClaw/ClawX）静默跳过，不算错误。
pub(crate) fn ensure_openclaw_text_commands() -> Result<(), String> {
    let oc_path = openclaw_home().join("openclaw.json");
    if !oc_path.exists() {
        return Ok(());
    }
    let mut oc: Value = std::fs::read_to_string(&oc_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({}));
    if !oc.is_object() {
        return Ok(());
    }
    {
        let root = oc.as_object_mut().unwrap();
        let commands = root.entry("commands").or_insert_with(|| json!({}));
        let commands = commands.as_object_mut().ok_or("openclaw.json commands 不是对象")?;
        if commands.contains_key("text") {
            return Ok(());
        }
        commands.insert("text".into(), json!(true));
    }
    backup_once(&oc_path);
    atomic_write(&oc_path, serde_json::to_string_pretty(&oc).unwrap().as_bytes())
        .map_err(|e| format!("写 openclaw.json 失败: {e}"))
}

/// 写 agent 的 models.json：providers.<prov> = {baseUrl, api, apiKey, models}。
/// 合并现有，不动别的 provider。best-effort（失败静默）。
fn write_agent_models_json(path: &PathBuf, prov: &str, base: &str, key: &str, models: &[Value]) {
    let mut root: Value = std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({ "providers": {} }));
    if !root.is_object() {
        root = json!({ "providers": {} });
    }
    let obj = root.as_object_mut().unwrap();
    let providers = obj.entry("providers").or_insert_with(|| json!({}));
    if let Some(p) = providers.as_object_mut() {
        p.retain(|id, _| !is_managed_agent_provider_key(id) || id == prov);
        p.insert(
            prov.into(),
            json!({
                "baseUrl": base,
                "api": "openai-completions",
                "apiKey": key,             // agent models.json 这里放真实 key（ClawX 同款）
                "timeoutSeconds": OPENCLAW_MODEL_TIMEOUT_SECONDS,
                "models": models,
            }),
        );
    }
    let _ = atomic_write(path, serde_json::to_string_pretty(&root).unwrap().as_bytes());
}

/// 写 agent 的 auth-profiles.json：profiles["<prov>:default"] + order + lastGood（ClawX 同构）。
fn write_agent_auth_profiles(path: &PathBuf, prov: &str, key: &str) {
    let mut root: Value = std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({ "version": 1, "profiles": {}, "order": {}, "lastGood": {} }));
    if !root.is_object() {
        root = json!({ "version": 1, "profiles": {}, "order": {}, "lastGood": {} });
    }
    let obj = root.as_object_mut().unwrap();
    obj.entry("version").or_insert_with(|| json!(1));
    let profile_id = format!("{prov}:default");

    let profiles = obj.entry("profiles").or_insert_with(|| json!({}));
    if let Some(p) = profiles.as_object_mut() {
        p.retain(|id, _| {
            id.strip_suffix(":default")
                .map(|provider| !is_managed_agent_provider_key(provider) || provider == prov)
                .unwrap_or(true)
        });
        p.insert(
            profile_id.clone(),
            json!({ "type": "api_key", "provider": prov, "key": key }),
        );
    }
    let order = obj.entry("order").or_insert_with(|| json!({}));
    if let Some(o) = order.as_object_mut() {
        o.retain(|id, _| !is_managed_agent_provider_key(id) || id == prov);
        o.insert(prov.into(), json!([profile_id.clone()]));
    }
    let last_good = obj.entry("lastGood").or_insert_with(|| json!({}));
    if let Some(l) = last_good.as_object_mut() {
        l.retain(|id, _| !is_managed_agent_provider_key(id) || id == prov);
        l.insert(prov.into(), json!(profile_id));
    }
    let _ = atomic_write(path, serde_json::to_string_pretty(&root).unwrap().as_bytes());
}

/// 还原 ClawX：删掉 `uking-*` 托管账号 + key + secret，默认指向另一个还存在的账号。
fn reset_clawx() -> Result<(), String> {
    let path = clawx_providers_path();
    let Ok(s) = std::fs::read_to_string(&path) else {
        return Ok(());
    };
    let Ok(mut root) = serde_json::from_str::<Value>(&s) else {
        return Ok(());
    };
    if let Some(obj) = root.as_object_mut() {
        let managed_ids: Vec<String> = obj
            .get("providerAccounts")
            .and_then(Value::as_object)
            .map(|a| a.keys().filter(|id| is_managed_provider_id(id)).cloned().collect())
            .unwrap_or_default();
        if let Some(a) = obj.get_mut("providerAccounts").and_then(|x| x.as_object_mut()) {
            a.retain(|id, _| !is_managed_provider_id(id));
        }
        if let Some(k) = obj.get_mut("apiKeys").and_then(|x| x.as_object_mut()) {
            for id in &managed_ids {
                k.remove(id);
            }
        }
        if let Some(sec) = obj.get_mut("providerSecrets").and_then(|x| x.as_object_mut()) {
            for id in &managed_ids {
                sec.remove(id);
            }
        }
        // 默认若还指向我们，改指向剩下任一账号（没有就清空）
        let still_default = obj
            .get("defaultProvider")
            .and_then(Value::as_str)
            .is_some_and(is_managed_provider_id);
        if still_default {
            let fallback = obj
                .get("providerAccounts")
                .and_then(|a| a.as_object())
                .and_then(|a| a.keys().next().cloned());
            match fallback {
                Some(id) => {
                    obj.insert("defaultProvider".into(), json!(id));
                    obj.insert("defaultProviderAccountId".into(), json!(id));
                }
                None => {
                    obj.remove("defaultProvider");
                    obj.remove("defaultProviderAccountId");
                }
            }
        }
    }
    atomic_write(&path, serde_json::to_string_pretty(&root).unwrap().as_bytes())
        .map_err(|e| format!("写 clawx-providers.json 失败: {e}"))
}

// ============================================================
// 当前状态（回显给前端）
// ============================================================

#[derive(Debug, Clone, Serialize, Default)]
pub struct DriverStatus {
    pub claude_base: Option<String>,
    pub claude_model: Option<String>,
    pub codex_provider: Option<String>,
    pub codex_model: Option<String>,
    /// ClawX（~/.openclaw/openclaw.json）当前默认模型；None = 未被我们接管
    pub clawx_model: Option<String>,
    /// ClawX 是否已装（探测到才在 UI 显示接管状态）
    pub clawx_installed: bool,
    /// Hermes（~/.hermes/config.yaml）当前默认模型；None = 未配置/未接管
    pub hermes_model: Option<String>,
    /// Hermes 是否已装（~/.hermes 存在）
    pub hermes_installed: bool,
    /// DSH Web / terminal 共用的当前默认模型。
    pub dsh_model: Option<String>,
    /// DSH CLI 是否已安装。
    pub dsh_installed: bool,
    /// **每个工具当前生效的 provider id**（对齐 cc-switch 的 is_current）。
    /// 显式记录优先（~/.uking/active-drivers.json），老安装无记录时按实时配置兜底推断。
    /// 前端回显「使用中」**只读这张表**，不再各自反推（Hermes 老 bug 根治）。
    /// 键 ∈ claude|codex|clawx|hermes，值 = provider id（official=还原官方）。
    pub active: std::collections::BTreeMap<String, String>,
    /// Claude Code 当前是不是走**本机的翻译桥**（`ANTHROPIC_BASE_URL` 指着 127.0.0.1）。
    ///
    /// 为什么单列一位而不是让前端自己看 `claude_base`：这条链路多了一环 **U-King 自己**，
    /// U-King 一退桥就没了、Claude Code 当场连不上。这是客户必须看得见的事实，
    /// 藏在一个 URL 里等人自己认出来就等于没说（同 `runs_only_while_app_open` 的道理）。
    pub claude_via_bridge: bool,
    /// 用户是否在用「自己的」Claude 配置（官方 OAuth 登录 / 自备 Key / 自己的中转，非虾盘云）。
    /// 为真 → 前端不推虾盘云、不弹接管提示，接入改成需明确点击并可一键还原。
    /// 铁律（CLAUDE.md 第 10 条）：绝不静默覆盖、绝不抢用户自己的 Key。
    pub claude_own_key: bool,
    /// 同上，Codex 侧（官方 ChatGPT 登录 / 用户自己的 config.toml，非 U-King 写的虾盘云）。
    pub codex_own_key: bool,
    /// ★ 后上架的那批 CLI（`pi` / `qwen` / `crush` / `opencode`）**装没装**。
    ///
    /// 为什么必须单独报出来：`apply_xiapan_everywhere` 早就会配这四个了，
    /// 而「一键配置」的弹窗只认识 claude/codex/clawx/hermes —— 它传上来的 `only` 名单
    /// 里永远没有这四个，于是**后端支持、界面看不见、客户永远配不上**。
    /// 界面列的和后端真会去配的必须是同一批，所以这里的判据就是那边用的
    /// `installer::tool_installed`，不另写一份探测。
    pub extra_installed: std::collections::BTreeMap<String, bool>,
}

/// 「一键配好全部」认得、但不在 `LIST_TOOLS` 独立列表页里的工具。
///
/// 🔴 **这份数组现在跟 `apply_xiapan_everywhere` 尾部那张表不是同一批 id 了**：
/// 尾表（约第 1409 行）仍然遍历 `pi`/`qwen`/`crush`/`opencode` 四个，这份数组只剩三个——
/// `pi` 已经不在这里。别再假设两边同步，改一处不代表另一处也要改。
///
/// 2026-08-22：`pi` 挪去了 `LIST_TOOLS`（它现在有自己的 Tab 了）。它从这份数组里移出，是因为
/// `APPLY_ALL_TARGETS` 的顺序必须严格等于 `LIST_TOOLS ++ EXTRA_APPLY_TOOLS`（见
/// `apply_everywhere_contract_lists_every_target_the_backend_configures` 用例），pi 挪去
/// LIST_TOOLS 末尾后正好接上 EXTRA_APPLY_TOOLS 的头，顺序不变、用例照样绿。但 `pi` 在
/// `apply_xiapan_everywhere` 里**照样会被配置**（尾表那份循环没有跟着删）；`driver_status()`
/// 里也给它单独补了一份 `extra_installed` / `active` 记录（不靠这份数组带出来），因为
/// 「一键配好全部」弹窗（`ApplyScopeDialog.tsx`）一直靠 `extra_installed` 判要不要列出 pi
/// 这一行——挪出数组会让它从弹窗里静默消失，两处入口（独立 Tab + 弹窗）都要保留。
///
/// 换句话说：**数组管「进不进 `APPLY_ALL_TARGETS` 契约 enum」，尾表管「真配不配」，
/// `driver_status()` 那两行管「弹窗看不看得见」——三件事目前各管各的，pi 是唯一一个
/// 三处规则不一致还必须保持一致效果的工具。**
/// 2026-08-24：`opencode` 也挪去了 `LIST_TOOLS`（它现在有自己的 Tab）。同 pi 的处理，
/// 见下面的 [`PROMOTED_TO_LIST_TOOLS`]。
/// 2026-08-29：`cline` 上架即走 pi/opencode 同款路径 —— 直接进 `LIST_TOOLS` +
/// 本数组 + 尾表 + `driver_status()` 四处，不重复 pi 当年「先进这数组、后升级」的两段式。
pub const EXTRA_APPLY_TOOLS: &[&str] = &["qwen", "crush"];

/// 从 [`EXTRA_APPLY_TOOLS`] **升上去**到 [`LIST_TOOLS`]（有了自己的 Tab）、
/// 但「一键配好全部」弹窗仍要靠 `extra_installed` / `active` 回显的工具。
///
/// 🔴 为什么要有这份数组：`ApplyScopeDialog.tsx` 判「这一行要不要列出来」看的是
/// `driver_status().extra_installed`，而那张表只对 `EXTRA_APPLY_TOOLS` 填充。
/// 一个工具从 `EXTRA_APPLY_TOOLS` 挪走 = **从弹窗里静默消失**（两处入口只剩一处）。
/// pi 那次是在 `driver_status()` 里硬写了两行补上的；opencode 再来一次就会是第二份复制。
/// 抽成数组之后，下一个工具升级只要在这里加一个词。
pub const PROMOTED_TO_LIST_TOOLS: &[&str] = &["pi", "opencode", "cline"];

/// 🔴 **「一键配好全部」真正会配的全部目标 —— 动作契约 `targets` 那份 enum 的唯一真相源。**
///
/// 为什么单独立一个常量：0.9.99 之前这批 id 在三个地方各写了一份 ——
/// 后端分派表（8 个）、动作契约 enum（4 个）、前端弹窗（4 个）。
/// 三份就漂了三份：后端早就会配 pi/qwen/crush/opencode，契约却对外声明不支持，
/// 前端连勾都勾不出来。**「一键配好全部」这几个字对这四个工具整整是假的。**
///
/// 契约 enum 现在由 `lib.rs` 直接读这里生成，前端由 `driver_status().extra_installed`
/// 驱动 —— 谁都不再手抄一份。`lib.rs` 里的
/// `apply_everywhere_contract_lists_every_target_the_backend_configures` 用例守着两件事：
/// ①契约 enum == `APPLY_ALL_TARGETS`；②`APPLY_ALL_TARGETS` == `LIST_TOOLS ++ EXTRA_APPLY_TOOLS`。
///
/// 🔴 **这条用例守的是常量之间的关系，不是「清单 vs 真正的分派表」**——它从不去看
/// `apply_xiapan_everywhere` 那个函数体到底配置了哪些工具。真正的缺口：把 `apply_xiapan_everywhere`
/// 尾表（约第 1409 行）里 `("pi", ...)` 那一行整个删掉，`cargo test` 一条都不会红，因为
/// 测试环境里 `pi` 本来就没装，`tool_installed` 判假就静默跳过——少配一个工具和「这个工具在这台
/// 机器上没装」长得一模一样。这个用例只能挡「常量清单互相漂移」，挡不住「常量写对了、
/// 循环体没跟着写」。缺一条断言分派表本身确实遍历了 `APPLY_ALL_TARGETS` 每一项（已记进需求榜）。
pub const APPLY_ALL_TARGETS: &[&str] = &[
    "claude", "codex", "clawx", "hermes", "dsh", "pi", "opencode", "cline", "qwen", "crush",
];

/// 「我们写的配置，那个工具真的会照着跑吗」—— **回读工具自己的配置文件**，解析出它启动时
/// 实际会用的 provider / model / 端点。
///
/// # 为什么必须有这层
///
/// 在这之前，「切换成功」的唯一凭据是 `atomic_write` 之后**逐字节回读比对**
/// （`write_verified`）。那条只证明「文件里是我写的内容」，证明不了
/// **「那个工具会读这个字段」**。2026-08-24 一天之内撞到三条它必然放行的：
///   · pi —— 我们写 `defaultModel`，而说了算的是 `defaultProvider`（没写）
///   · opencode —— 我们写 `opencode.json`，而同目录的 `opencode.jsonc` 压过它
///   · codex —— `disable_response_storage` 是 unknown field，`--strict-config` 下整份拒载
/// 三条的共同形状是**「报告是对的，世界是坏的」**：GUI 说已切到 DeepSeek，工具跑的是 kimi。
/// 客户的归因必然是「这软件的设置不准」，而我们这边**零信号**。
///
/// # 铁律：不知道就说不知道
///
/// `readable == false` 表示**我们没有这个工具的回读路径**，不是「读了但没配」。
/// 前端必须把这两种渲染成不同的东西 —— 空结果有两义，把「没查」显示成绿勾，
/// 就是 `--install-test-cjk` 那类「报告是对的、世界是坏的」的又一份（CLAUDE.md readiness 条）。
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct EffectiveConfig {
    pub target: String,
    /// 我们有没有这个工具的回读路径。false = 不知道，别渲染成任何结论。
    pub readable: bool,
    /// 工具配置里当前选中的 provider 键名（不是我们的 preset id）。
    pub provider_key: Option<String>,
    /// 该 provider 的端点（能读到才给）。
    pub base_url: Option<String>,
    /// 工具启动时实际会用的模型。
    pub model: Option<String>,
    /// 🔴 **有别的东西压着我们写的那份** —— 写入成功但不生效。给出压着它的文件路径。
    pub overridden_by: Option<String>,
}

impl EffectiveConfig {
    fn unknown(target: &str) -> Self {
        Self { target: target.into(), readable: false, ..Default::default() }
    }
}

/// 回读某个配置目标当前**真正生效**的配置。见 [`EffectiveConfig`] 的文档。
pub fn effective_config(target: &str) -> EffectiveConfig {
    let mut r = EffectiveConfig { target: target.into(), readable: true, ..Default::default() };
    match target {
        "claude" => {
            let Ok(s) = std::fs::read_to_string(claude_settings_path()) else {
                return r; // 文件不存在 = 读得到「没配」，仍算 readable
            };
            let Ok(v) = serde_json::from_str::<Value>(&s) else {
                return EffectiveConfig::unknown(target);
            };
            let env = v.get("env");
            r.base_url = env
                .and_then(|e| e.get("ANTHROPIC_BASE_URL"))
                .and_then(|x| x.as_str())
                .map(str::to_string);
            r.model =
                env.and_then(|e| e.get("ANTHROPIC_MODEL")).and_then(|x| x.as_str()).map(str::to_string);
            r.provider_key = id_from_base(r.base_url.as_deref());
        }
        "codex" => {
            let Ok(s) = std::fs::read_to_string(codex_dir().join("config.toml")) else {
                return r;
            };
            // config.toml 只需要三个键，用现成的行扫描，不为此引 toml crate（体积优先）。
            r.model = toml_top_level_string(&s, "model");
            r.provider_key = toml_top_level_string(&s, "model_provider");
            if let Some(p) = r.provider_key.as_deref() {
                r.base_url = toml_table_string(&s, &format!("model_providers.{p}"), "base_url");
            }
        }
        "pi" => {
            let Ok(s) = std::fs::read_to_string(pi_settings_path()) else {
                return r;
            };
            let Ok(v) = serde_json::from_str::<Value>(&s) else {
                return EffectiveConfig::unknown(target);
            };
            // 🔴 顺序就是 pi 自己的判据：`defaultProvider` 说了算，`defaultModel` 只在它内部匹配。
            // 四组变异实测见 `apply_pi` 函数头 —— 这里**必须跟那边同一个口径**，
            // 否则回验会把「写对了」和「跑对了」再次错开。
            r.provider_key = v.get("defaultProvider").and_then(|x| x.as_str()).map(str::to_string);
            r.model = v.get("defaultModel").and_then(|x| x.as_str()).map(str::to_string);
            if let (Some(p), Ok(ms)) =
                (r.provider_key.as_deref(), std::fs::read_to_string(pi_models_path()))
            {
                if let Ok(mv) = serde_json::from_str::<Value>(&ms) {
                    r.base_url = mv
                        .pointer(&format!("/providers/{p}/baseUrl"))
                        .and_then(|x| x.as_str())
                        .map(str::to_string);
                }
            }
        }
        "opencode" => {
            let json = opencode_config_path();
            let jsonc = json.with_extension("jsonc");
            let read = |p: &std::path::Path| -> Option<Value> {
                serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()
            };
            let base = read(&json);
            let over = read(&jsonc);
            // 实测（沙箱 `opencode debug config`）：provider 合并，**顶层 model 由 jsonc 说了算**。
            let model_from_jsonc = over.as_ref().and_then(|v| v.get("model")).and_then(|x| x.as_str());
            let model_from_json = base.as_ref().and_then(|v| v.get("model")).and_then(|x| x.as_str());
            // 🔴 **只有「不一致」才叫被压着。** 第一版这里写的是「jsonc 里有 model 就算被压」，
            // 结果切换成功、两份已经对齐之后，界面**照样弹橙色警告** —— 一个在正常状态下
            // 长期亮着的警告，两天之内就会被用户学会无视，那这条警告以后真响的时候也没人看。
            // 判据必须是「它跟我们写的那份不一样」，不是「它存在」。
            if model_from_jsonc.is_some() && model_from_jsonc != model_from_json {
                r.overridden_by = Some(jsonc.display().to_string());
            } else if jsonc.exists() && over.is_none() {
                // jsonc 在、但严格 JSON 解析不动（带注释）→ 它可能钉着 model，我们看不见。
                // **不许猜**：整条判成不知道，并如实报出是哪个文件挡住了视线。
                let mut u = EffectiveConfig::unknown(target);
                u.overridden_by = Some(jsonc.display().to_string());
                return u;
            }
            let model = model_from_jsonc
                .map(str::to_string)
                .or_else(|| base.as_ref()?.get("model")?.as_str().map(str::to_string));
            // 顶层 model 形如 `<provider>/<model>`
            if let Some(m) = model.as_deref() {
                if let Some((p, rest)) = m.split_once('/') {
                    r.provider_key = Some(p.to_string());
                    r.model = Some(rest.to_string());
                } else {
                    r.model = Some(m.to_string());
                }
            }
            if let (Some(p), Some(b)) = (r.provider_key.as_deref(), base.as_ref()) {
                r.base_url = b
                    .pointer(&format!("/provider/{p}/options/baseURL"))
                    .and_then(|x| x.as_str())
                    .map(str::to_string);
            }
        }
        "hermes" => {
            let Ok(s) = std::fs::read_to_string(hermes_dir().join("config.yaml")) else {
                return r;
            };
            r.model = yaml_model_default(&s);
            r.base_url = read_hermes_model_key(&s, "base_url");
            r.provider_key = id_from_base(r.base_url.as_deref());
        }
        // Cline：回读我们自己写的 `openai-compatible` 槽位（apply_cline 的权威 schema）。
        // `provider_key` 是 Cline 内置 id（不是 U-King 的 preset id），端点对上虾盘云时
        // 由 `id_from_base` 反推成 preset id 供前端显示；lastUsedProvider 指别处时如实报。
        "cline" => {
            let Ok(s) = std::fs::read_to_string(cline_providers_path()) else {
                return r;
            };
            let Ok(v) = serde_json::from_str::<Value>(&s) else {
                return EffectiveConfig::unknown(target);
            };
            let active = v
                .get("lastUsedProvider")
                .and_then(|x| x.as_str())
                .filter(|id| *id == CLINE_PROVIDER_KEY);
            if active.is_none() {
                // 用户自己在 Cline 里切去了别的 provider = 别人的路由，回读如实报「不指我们」。
                r.provider_key = v.get("lastUsedProvider").and_then(|x| x.as_str()).map(str::to_string);
                return r;
            }
            let slot = v.pointer(&format!("/providers/{CLINE_PROVIDER_KEY}/settings"));
            r.model = slot.and_then(|s| s.get("model")).and_then(|x| x.as_str()).map(str::to_string);
            r.base_url = slot
                .and_then(|s| s.get("baseUrl"))
                .and_then(|x| x.as_str())
                .map(str::to_string);
            r.provider_key = id_from_base(r.base_url.as_deref()).or(Some(CLINE_PROVIDER_KEY.into()));
        }
        "dsh" => {
            let (prov, model, base) = dsh_live_selection();
            r.provider_key = prov;
            r.model = model;
            r.base_url = base;
        }
        // clawx 仍无独立回读（driver_status 已单独读它）；其余工具都有回读。读不动
        // 的文件如实返回 unknown——一个读错的回验比没有回验更坏（它会给出一个
        // 可信的错结论）。
        "qwen" => {
            // Qwen Code：`~/.qwen/settings.json`。模型 = `model.name`（apply_qwen 写入处
            // 同一键）。端点 = `modelProviders.openai[]` 里带我们托管 envKey 的那项的
            // baseUrl —— 用 envKey 认领而不是扫 baseUrl，避免客户自建条目被误认成我们的。
            let Ok(s) = std::fs::read_to_string(qwen_settings_path()) else {
                return r;
            };
            let Ok(v) = serde_json::from_str::<Value>(&s) else {
                return EffectiveConfig::unknown(target);
            };
            r.model = v
                .pointer("/model/name")
                .and_then(|x| x.as_str())
                .map(str::to_string);
            if let Some(list) = v.pointer("/modelProviders/openai").and_then(|x| x.as_array()) {
                for e in list {
                    let is_ours = matches!(
                        e.get("envKey").and_then(|k| k.as_str()),
                        Some(UKING_ENV_KEY) | Some(LEGACY_UKING_ENV_KEY)
                    );
                    if is_ours {
                        r.base_url =
                            e.get("baseUrl").and_then(|x| x.as_str()).map(str::to_string);
                        break;
                    }
                }
            }
        }
        "crush" => {
            // Crush：`~/.config/crush/crush.json`。模型 = `models.large.model`
            // （apply_crush 把 large/small 都写上，large 是主槽）。端点 = 该模型引用的
            // provider 的 base_url；provider 不在我们手里（非托管 id）时端点留空，
            // 由上层「base_url 非虾盘云 → 别家的路由」判定兜住。
            let Ok(s) = std::fs::read_to_string(crush_config_path()) else {
                return r;
            };
            let Ok(v) = serde_json::from_str::<Value>(&s) else {
                return EffectiveConfig::unknown(target);
            };
            let slot = v.pointer("/models/large/model").and_then(|x| x.as_str());
            r.model = slot.map(str::to_string);
            if let Some(pid) = v.pointer("/models/large/provider").and_then(|x| x.as_str()) {
                r.provider_key = Some(pid.to_string());
                // 端点照实回读，不管 provider 是不是我们托管的 —— 上层要靠它判
                // 「这个工具当前走的是谁」：非托管 + 非虾盘云端点 = 客户自己的路由。
                r.base_url = v
                    .pointer(&format!("/providers/{pid}/base_url"))
                    .and_then(|x| x.as_str())
                    .map(str::to_string);
            }
        }        // clawx：apply_clawx 全量接管 + driver_status 单独回显，这里没有独立回读。
        _ => return EffectiveConfig::unknown(target),
    }
    r
}

/// config.toml 顶层的 `key = "value"`（只认到第一个 `[section]` 为止）。
fn toml_top_level_string(text: &str, key: &str) -> Option<String> {
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            break;
        }
        if let Some(v) = toml_kv(t, key) {
            return Some(v);
        }
    }
    None
}

/// config.toml 里 `[section]` 块内的 `key = "value"`。
fn toml_table_string(text: &str, section: &str, key: &str) -> Option<String> {
    let mut inside = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            inside = t.trim_start_matches('[').trim_end_matches(']').trim() == section;
            continue;
        }
        if inside {
            if let Some(v) = toml_kv(t, key) {
                return Some(v);
            }
        }
    }
    None
}

fn toml_kv(line: &str, key: &str) -> Option<String> {
    let rest = line.strip_prefix(key)?.trim_start();
    let rest = rest.strip_prefix('=')?.trim();
    Some(rest.trim_matches('"').to_string())
}

pub fn driver_status() -> DriverStatus {
    let mut st = DriverStatus::default();
    if let Ok(s) = std::fs::read_to_string(claude_settings_path()) {
        if let Ok(v) = serde_json::from_str::<Value>(&s) {
            let env = v.get("env");
            st.claude_base = env
                .and_then(|e| e.get("ANTHROPIC_BASE_URL"))
                .and_then(|x| x.as_str())
                // 空串/非法 URL 视为「没配」—— 历史 bug 写坏过 ANTHROPIC_BASE_URL="",
                // 这种值会让 claude 启动失败。当成未配置，触发自动接虾盘云覆盖修复。
                .filter(|s| s.starts_with("http://") || s.starts_with("https://"))
                .map(String::from);
            st.claude_model = env
                .and_then(|e| e.get("ANTHROPIC_MODEL"))
                .and_then(|x| x.as_str())
                .map(String::from);
            // 只认「指向本机」这个事实，不写死端口 —— 端口是 `claude_proxy` 的，
            // 抄一份过来就会漂移（宪法第 8 条）。而且不管哪个本地代理，
            // 「多了一环、那一环挂了就断」这句话都成立。
            st.claude_via_bridge = st
                .claude_base
                .as_deref()
                .map(|b| b.starts_with("http://127.0.0.1:") || b.starts_with("http://localhost:"))
                .unwrap_or(false);
        }
    }
    if let Ok(s) = std::fs::read_to_string(codex_dir().join("config.toml")) {
        for line in s.lines() {
            let line = line.trim();
            if let Some(v) = line.strip_prefix("model_provider") {
                st.codex_provider = toml_str_value(v);
            } else if let Some(v) = line.strip_prefix("model ") {
                st.codex_model = toml_str_value(v);
            } else if let Some(v) = line.strip_prefix("model=") {
                st.codex_model = toml_str_value(&format!("={v}"));
            }
        }
    }
    // ClawX 4.x 当前模型：读 clawx-providers.json，只有默认指向我们的 uking-* 账号
    // 才算「已被 U-King 接管」，回显该账号的 model。
    //
    // `clawx_default_is_ours` 是**三态，别塌成 bool**：
    //   Some(true)  = 默认就是我们的账号
    //   Some(false) = 客户自己在 ClawX 界面里选了别家（我们没有对应的 preset id）
    //   None        = 配置读不到 / 还没写过 defaultProvider —— 是**不知道**，不是「不是」
    let mut clawx_default_is_ours: Option<bool> = None;
    let mut clawx_live_provider_id: Option<String> = None;
    if let Ok(s) = std::fs::read_to_string(clawx_providers_path()) {
        if let Ok(v) = serde_json::from_str::<Value>(&s) {
            let default_id = v.get("defaultProvider").and_then(|x| x.as_str());
            if let Some(id) = default_id {
                clawx_default_is_ours = Some(is_managed_provider_id(id));
                if is_managed_provider_id(id) {
                    clawx_live_provider_id = provider_id_from_managed_route(id);
                }
            }
            if let Some(id) = default_id.filter(|id| is_managed_provider_id(id)) {
                st.clawx_model = v
                    .get("providerAccounts")
                    .and_then(|a| a.get(id))
                    .and_then(|a| a.get("model"))
                    .and_then(|x| x.as_str())
                    .map(String::from);
            }
        }
    }
    st.clawx_installed = clawx_app_installed();

    // Hermes：~/.hermes 存在算已装；读 config.yaml 的 model.default 回显当前模型 +
    // model.base_url（用于「无显式记录」时按 base 反推生效的 provider，修正老 bug）。
    let hdir = hermes_dir();
    st.hermes_installed = hdir.exists() || crate::installer::tool_installed("hermes");
    let mut hermes_base: Option<String> = None;
    if let Ok(s) = std::fs::read_to_string(hdir.join("config.yaml")) {
        st.hermes_model = yaml_model_default(&s);
        hermes_base = yaml_model_field(&s, "base_url");
    }

    // DSH 的真状态以 settings.yaml 为准：用户可在 Web 模型页直接切换，
    // 若只读 active-drivers.json，U-King 会在客户已经切走后仍假报「正在用虾盘云」。
    let (dsh_provider, dsh_model, dsh_base) = dsh_live_selection();
    st.dsh_model = dsh_model;
    st.dsh_installed = crate::installer::tool_installed("dsh");

    // 当前生效记录（对齐 cc-switch is_current）：
    // - **claude / codex：以活配置为准**（读 ~/.claude/settings.json 的 base_url、~/.codex 的
    //   model_provider）。活配置才是「当前用哪个驱动」的单一真相源——这样在 uu-switch / 手动
    //   改了底层配置后，U-King 回显能**跟着同步**；能推出已知驱动就用它，推不出（官方直连/未知
    //   中转）再回退 active-drivers.json 记录。
    // - **clawx / hermes：仍显式记录优先**（它们的 base 反推口径历史上有坑——Hermes 老 bug），
    //   无记录再按实时配置兜底，维持原行为不动。
    let recorded = load_active_drivers();
    let recorded_of = |tool: &str| recorded.get(tool).and_then(|x| x.as_str()).map(String::from);
    for tool in ["claude", "codex", "clawx", "hermes", "dsh"] {
        let val = match tool {
            // 活配置权威（同步 uu-switch / 外部改动），推不出再回退记录
            "claude" => id_from_base(st.claude_base.as_deref()).or_else(|| recorded_of(tool)),
            // Codex 的 model_provider 直接就是 provider id（apply_codex 写的）
            "codex" => st
                .codex_provider
                .clone()
                .filter(|v| ["xiapan", "deepseek", "glm", "kimi"].contains(&v.as_str()))
                .or_else(|| recorded_of(tool)),
            // ClawX：**活配置优先**（对齐 claude / codex / dsh 的口径）。
            //
            // 🔴 2026-08-21 pc-***：这里原来是「显式记录优先」。可客户在 **ClawX 自己的界面里**
            // 换供应商，是不会回头更新我们 `active-drivers.json` 的 —— 记录就永远停在 xiapan。
            // 两层后果：① 对着一台已经切走的机器回显「虾盘云 · 使用中」；② 设备钱包换 Key 写回
            // 时据此判成「ClawX 还是我们的」，一路调到 `apply_clawx`，把 `defaultProvider` 钉回
            // uking-xiapan、其它账号 isDefault 全置 false —— 客户配好的小米 MiMo 被反复顶掉，
            // 他看到的是「配不了自己的模型，只能用虾盘云」。**陈旧记录不该有权改写活配置。**
            //
            // 切到别家时我们没有对应的 preset id，于是**推不出来就不写**（同下面那批的口径）：
            // 宁可界面显示「未配置」，也不要编一个「已配：虾盘云」。
            "clawx" => match clawx_default_is_ours {
                // 默认是我们的账号 = **U-King 配的**。账号 id 现在本身就携带真实
                // 上游，所以活配置反查结果优先；旧 active-drivers 只作兼容兜底。
                // 这样即使记录缺失/陈旧，`uking-openrouter` 也不会再被谎报成 xiapan。
                Some(true) => clawx_live_provider_id
                    .clone()
                    .or_else(|| recorded_of(tool))
                    .or_else(|| Some("xiapan".to_string())),
                Some(false) => None,
                None => recorded_of(tool),
            },
            // Hermes：显式记录优先；无记录再按 base 反推
            "hermes" => recorded_of(tool).or_else(|| id_from_base(hermes_base.as_deref())),
            // DSH：活配置优先。用户在 DSH Web 里切到其它 route 后，立即尊重那个选择。
            // 只有仍指向 U-King route 时，才用 endpoint / 我们的记录反推具体 preset。
            "dsh" => match dsh_provider.as_deref() {
                Some(route) if is_managed_provider_id(route) => id_from_base(dsh_base.as_deref())
                    .or_else(|| provider_id_from_managed_route(route))
                    .or_else(|| recorded_of(tool))
                    .or_else(|| Some(route.to_string())),
                Some(other) => Some(other.to_string()),
                None => recorded_of(tool),
            },
            _ => None,
        };
        if let Some(v) = val {
            st.active.insert(tool.to_string(), v);
        }
    }

    // 后上架的那批（qwen/crush/opencode）：装没装 + 当前记的是哪个 provider。
    // 它们没有各自的「读活配置反推」路径（配置格式各不相同），所以只认显式记录 ——
    // **推不出来就不写**，宁可界面显示「未配置」，也不要编一个「已配：xxx」。
    for tool in EXTRA_APPLY_TOOLS {
        st.extra_installed
            .insert((*tool).to_string(), crate::installer::tool_installed(tool));
        if let Some(v) = recorded_of(tool) {
            st.active.insert((*tool).to_string(), v);
        }
    }
    // 升进 `LIST_TOOLS` 的那批（pi / opencode）也要补同样的记录：它们现在有自己的
    // provider 列表 Tab，但「一键配好全部」弹窗（`ApplyScopeDialog.tsx` 的 `EXTRA_TOOLS`
    // 列表）一直靠 `extra_installed`/`active` 判要不要列出它们——**两处入口都要保留**，
    // 少这一段它们就从弹窗里静默消失。清单见 `PROMOTED_TO_LIST_TOOLS`（原来是给 pi 硬写的两行）。
    for tool in PROMOTED_TO_LIST_TOOLS {
        st.extra_installed
            .insert((*tool).to_string(), crate::installer::tool_installed(tool));
        if let Some(v) = recorded_of(tool) {
            st.active.insert((*tool).to_string(), v);
        }
    }

    // 「用户在用自己的 Key」判据 —— 铁律：探测到就绝不推虾盘云、绝不抢。
    st.claude_own_key = claude_owns_config();
    st.codex_own_key = codex_owns_config();
    st
}

/// 用户的 Claude 是否是「自己的」配置（非 U-King 虾盘云）。
///
/// 判据（任一成立即为真；拿不准一律当「是自己的」—— 宁可不推虾盘云，也绝不抢用户 Key）：
///  ① `~/.claude/.credentials.json` 存在且非空 → 官方 OAuth 登录（用户自己的 Claude 账号）
///  ② settings.json 的 env 有非空 `ANTHROPIC_AUTH_TOKEN`，且 base 不含 `u-claw.org`
///     → 自备 Key / 自己的中转（DeepSeek / GLM / 自建代理都算）
fn claude_owns_config() -> bool {
    // ① 官方 OAuth 登录：Claude Code 的凭据文件（>8 字节才算真有内容，空 `{}` 不算）
    let cred = config_home().join(".claude").join(".credentials.json");
    if std::fs::metadata(&cred).map(|m| m.len() > 8).unwrap_or(false) {
        return true;
    }
    // ② 自备 Key，且端点不是我们的虾盘云
    if let Ok(s) = std::fs::read_to_string(claude_settings_path()) {
        if let Ok(v) = serde_json::from_str::<Value>(&s) {
            let env = v.get("env");
            let token = env
                .and_then(|e| e.get("ANTHROPIC_AUTH_TOKEN"))
                .and_then(|x| x.as_str())
                .unwrap_or("");
            let base = env
                .and_then(|e| e.get("ANTHROPIC_BASE_URL"))
                .and_then(|x| x.as_str())
                .unwrap_or("");
            if !token.trim().is_empty() && !is_xiapan_endpoint(&base) {
                return true;
            }
        }
    }
    false
}

/// 用户的 Codex 是否是「自己的」配置（非 U-King 写的虾盘云）。
///
/// 判据（任一成立即为真）：
///  ① `~/.codex/config.toml` 存在、非空，且**没有** `managed by U-King` 标记 → 用户自己写的配置
///  ② `~/.codex/auth.json` 里有 OAuth `tokens`（官方 ChatGPT 登录，不是我们写的裸 OPENAI_API_KEY）
fn codex_owns_config() -> bool {
    let dir = codex_dir();
    if let Ok(s) = std::fs::read_to_string(dir.join("config.toml")) {
        if !s.trim().is_empty() && !s.contains("managed by U-King") {
            return true;
        }
    }
    if let Ok(s) = std::fs::read_to_string(dir.join("auth.json")) {
        if let Ok(v) = serde_json::from_str::<Value>(&s) {
            if v.get("tokens").map(|t| !t.is_null()).unwrap_or(false) {
                return true;
            }
        }
    }
    false
}

/// 从 Hermes config.yaml 的 `model:` 块里取某个子键的值（朴素解析，避免引 yaml crate）。
/// 例：`yaml_model_field(text, "default")` / `yaml_model_field(text, "base_url")`。
fn yaml_model_field(text: &str, key: &str) -> Option<String> {
    let needle = format!("{key}:");
    let mut in_model = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        // 顶层键（无前导空格）切换 model 块
        if !line.starts_with(char::is_whitespace) && !line.trim().is_empty() {
            in_model = trimmed.starts_with("model:");
        }
        if in_model {
            if let Some(rest) = trimmed.strip_prefix(&needle) {
                let v = rest.trim().trim_matches('"').trim_matches('\'').to_string();
                if !v.is_empty() {
                    return Some(v);
                }
            }
        }
    }
    None
}

/// 取 model.default（回显当前 Hermes 模型）。
fn yaml_model_default(text: &str) -> Option<String> {
    yaml_model_field(text, "default")
}

/// 绿色版/解压版 ClawX 探测：在常见落点（桌面/下载/各盘根）下找「直接含 ClawX.exe」
/// 或「ClawX* 子目录里含 ClawX.exe」的目录。只看一层子目录，避免深度遍历卡 IO。
fn portable_clawx_found(home: &str) -> bool {
    let mut roots: Vec<std::path::PathBuf> = vec![
        std::path::Path::new(home).join("Desktop"),
        std::path::Path::new(home).join("Downloads"),
        std::path::Path::new(home).to_path_buf(),
    ];
    // 各盘根（D:\ E:\ … U 盘常在这）。C 盘根不扫（噪声大），从 D 开始。
    #[cfg(windows)]
    for d in 'D'..='J' {
        roots.push(std::path::PathBuf::from(format!("{d}:\\")));
    }

    let has_exe = |dir: &std::path::Path| dir.join("ClawX.exe").exists();
    for root in roots {
        if !root.is_dir() {
            continue;
        }
        // 根下直接有 ClawX.exe？（解压到盘根的情况）
        if has_exe(&root) {
            return true;
        }
        // 根下一层子目录里，名字带 clawx 的，看有没有 ClawX.exe
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if !p.is_dir() {
                continue;
            }
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_lowercase();
            if name.contains("clawx") && has_exe(&p) {
                return true;
            }
        }
    }
    false
}

/// ClawX 桌面版是否已装。多路兜底（客户装在哪都尽量认出来，避免「装了还提示下载」）：
///  ① 常见安装目录（%LOCALAPPDATA%\Programs\ClawX 等）
///  ①.5 绿色版/解压版（桌面/下载/盘根的 ClawX*/ClawX.exe）
///  ② 开始菜单快捷方式（NSIS/Squirrel 默认建）
///  ③ 注册表卸载项（HKCU Uninstall 里有 ClawX）
pub fn clawx_app_installed() -> bool {
    // macOS：官方 dmg 拖进 /Applications（也兜底 ~/Applications）。老代码只查 Windows
    // 落点 → Mac 装了 ClawX 也显示「未安装」（一台 Mac 客户机实锤，2026-07-11）。
    #[cfg(target_os = "macos")]
    {
        let mac_home = std::env::var("HOME").unwrap_or_default();
        if std::path::Path::new("/Applications/ClawX.app").exists()
            || std::path::Path::new(&mac_home).join("Applications/ClawX.app").exists()
        {
            return true;
        }
    }
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
    let appdata = std::env::var("APPDATA").unwrap_or_default();
    // NSIS 默认「为所有用户安装」会装到 Program Files（实测客户机 C:\Program Files\ClawX\ClawX.exe）。
    // 老代码漏了这条 → 装了也识别不到（客户实测：装了 ClawX，U-King 还提示去下载）。
    let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".into());
    let pf86 = std::env::var("ProgramFiles(x86)").unwrap_or_else(|_| "C:\\Program Files (x86)".into());

    // ① 安装目录 + ② 开始菜单快捷方式
    let dir_hit = [
        std::path::Path::new(&pf).join("ClawX"),
        std::path::Path::new(&pf86).join("ClawX"),
        std::path::Path::new(&local).join("Programs").join("ClawX"),
        std::path::Path::new(&local).join("ClawX"),
        std::path::Path::new(&home).join("ClawX"),
        std::path::Path::new(&home).join("Desktop").join("ClawX"),
        std::path::Path::new(&appdata)
            .join("Microsoft\\Windows\\Start Menu\\Programs\\ClawX.lnk"),
        std::path::Path::new(&appdata)
            .join("Microsoft\\Windows\\Start Menu\\Programs\\ClawX"),
    ]
    .iter()
    .any(|p| p.exists());
    if dir_hit {
        return true;
    }

    // ①.5 绿色版/解压版 ClawX：用户解压到桌面/下载/任意盘根，文件夹名可能是
    //     ClawX-0.4.11-win-x64 之类，不叫 "ClawX"。扫常见落点下「直接含 ClawX.exe」或
    //     「ClawX*/ClawX.exe」的目录（只看一层，避免深扫卡 IO）。客户实测：装了绿色版
    //     U-King 检测不到、还提示下载，就是漏了这条。
    if portable_clawx_found(&home) {
        return true;
    }

    // ③ 注册表卸载项（Electron 安装器都会登记；reg query 失败/无此项时静默 false）。
    //    「为所有用户装」登记在 HKLM、「仅当前用户」在 HKCU —— 两个 hive 都查（漏 HKLM 是老 bug）。
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        for hive in [
            "HKLM\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
            "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
        ] {
            if let Ok(out) = std::process::Command::new(crate::installer::system_tool("reg"))
                .args(["query", hive, "/s", "/f", "ClawX"])
                .creation_flags(CREATE_NO_WINDOW)
                .output()
            {
                if out.status.success() && String::from_utf8_lossy(&out.stdout).contains("ClawX") {
                    return true;
                }
            }
        }
    }
    false
}

fn toml_str_value(rest: &str) -> Option<String> {
    rest.split('=')
        .nth(1)
        .map(|s| s.trim().trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
}

// ============================================================
// 连通性实测（让模型真回一句话）
// ============================================================

#[derive(Debug, Clone, Serialize)]
pub struct TestResult {
    pub ok: bool,
    pub api: String,
    pub latency_ms: u64,
    pub reply: Option<String>,
    pub error: Option<String>,
}

/// 实测驱动：`anthropic`（Claude Code）、`openai`（Codex Responses）、
/// `openai-chat`（DSH / Hermes / OpenCode / ClawX）。不能拿其中一条给另一条报绿。
pub fn test_provider(
    provider_id: &str,
    api_key: &str,
    model_override: Option<&str>,
    api: &str,
) -> TestResult {
    let p = match preset(provider_id) {
        Ok(p) => p,
        Err(e) => return test_err(api, 0, e),
    };
    let t0 = Instant::now();

    let r = match api {
        "anthropic" => {
            let Some(base) = p.anthropic_base.clone() else {
                return test_err(api, 0, format!("{} 不支持 Anthropic 格式", p.name));
            };
            let model = effective_model(&p, model_override);
            anthropic_chat(&base, api_key, &model, "请用一句中文确认你已就绪。")
        }
        // 其它 AI 工具走 OpenAI Chat Completions，别把 Codex 的 /responses 当作它们的验收。
        "openai-chat" => {
            let model = effective_model(&p, model_override);
            openai_chat(&p.openai_base, api_key, &model, "请用一句中文确认你已就绪。")
        }
        // openai = Codex 链路：测 Codex 实际会用的 Responses 端点和模型。
        _ => {
            let model = effective_codex_model(&p, model_override);
            openai_responses(&p.openai_base, api_key, &model, "请用一句中文确认你已就绪。")
        }
    };
    let ms = t0.elapsed().as_millis() as u64;

    match r {
        Ok(reply) => TestResult {
            ok: true,
            api: api.into(),
            latency_ms: ms,
            reply: Some(reply),
            error: None,
        },
        Err(e) => test_err(api, ms, e),
    }
}

/// 「添加供应商」弹窗里的**存前试连** —— 表单还没保存、没有 provider id，直接拿
/// 用户填到一半的 base/Key/model 打一发。
///
/// 🔴 为什么必须有这条（2026-08-22 用户亲历「无法准确添加新的供应商」）：原来唯一的
/// 试连是 `test_provider`，它要 `preset(provider_id)` —— 也就是**先保存才能测**。
/// 于是填错的人只能盲存，然后在列表深处、甚至切换驱动失败时才见到第一条报错，
/// 到那会儿报错已经离「你填错的那一格」隔了三层。存前能测，错误就死在弹窗里。
pub fn probe_openai_endpoint(base: &str, api_key: &str, model: &str) -> TestResult {
    let base = base.trim().trim_end_matches('/');
    if base.is_empty() {
        return test_err("openai", 0, "接口地址还没填".into());
    }
    if model.trim().is_empty() {
        return test_err("openai", 0, "先填一个模型 id（或点「拉取」从清单里选）".into());
    }
    let t0 = Instant::now();
    let r = openai_chat(base, api_key, model.trim(), "请用一句中文确认你已就绪。");
    let ms = t0.elapsed().as_millis() as u64;
    match r {
        Ok(reply) => TestResult { ok: true, api: "openai".into(), latency_ms: ms, reply: Some(reply), error: None },
        Err(e) => test_err("openai", ms, e),
    }
}

fn test_err(api: &str, ms: u64, e: String) -> TestResult {
    TestResult {
        ok: false,
        api: api.into(),
        latency_ms: ms,
        reply: None,
        error: Some(friendlier_text_error(e)),
    }
}

fn friendlier_text_error(e: String) -> String {
    let low = e.to_lowercase();
    if low.contains("quota is not enough") || low.contains("token quota") || low.contains("insufficient") {
        return "余额不足：当前虾盘云余额不够这次 Codex/聊天请求，请充值补足后再试。".to_string();
    }
    if low.contains("invalid token") || low.contains("unauthorized") || low.contains("401") {
        return "Key 暂时不可用：通常是这台电脑的虾盘云 Key 还未充值开通，或配置的 Key 不正确。请充值后刷新，仍不行再重新接入驱动。".to_string();
    }
    e
}

/// 动态拉取某 provider **真实可用的模型清单**（OpenAI 兼容 `GET {base}/models`）。
///
/// 对齐 cc-switch：模型不写死，从上游拉真实清单；UI 永远保留手填兜底，拉不到也能用。
/// - `provider_id`：内置或自定义；统一用其 `openai_base`（`/v1/models` 是 OpenAI 兼容标准，
///   虾盘云/DeepSeek/GLM/Kimi/Ollama/自定义中转都吃这套；纯 Anthropic 端点没有 models 列表）。
/// - `api_key`：前端传**已解析**的 key（虾盘云传内置 Key，自定义传自带 key，Ollama 传占位）。
///   空 key 也照发（本地 Ollama 不校验），由上游决定放不放行。
///
/// 失败一律返回 `Err`（前端据此退回内置候选 + 手填，绝不让换模型流程崩）。
pub fn list_remote_models(provider_id: &str, api_key: &str) -> Result<Vec<String>, String> {
    let p = preset(provider_id)?;
    let base = p.openai_base.trim().to_string();
    if base.trim_end_matches('/').is_empty() {
        return Err(format!("{} 没有 OpenAI 兼容端点，无法拉取模型清单（请手填模型 id）", p.name));
    }
    list_models_at(&base, api_key)
}

/// 按**裸 base URL** 拉 `/models` 清单 —— 给「添加供应商」弹窗的存前拉取用
/// （表单没保存就没有 provider id，`list_remote_models` 那条走不了）。
/// 逻辑就是原来 `list_remote_models` 的后半截，抽出来两边共用，不复制第二份。
pub fn list_models_at(base_url: &str, api_key: &str) -> Result<Vec<String>, String> {
    let base = base_url.trim().trim_end_matches('/').to_string();
    if base.is_empty() {
        return Err("接口地址还没填".into());
    }
    let url = format!("{base}/models");
    let auth = format!("Authorization: Bearer {api_key}");
    let mut args: Vec<&str> = vec!["-sS", "-m", "20"];
    if !api_key.trim().is_empty() {
        args.push("-H");
        args.push(&auth);
    }
    args.push(&url);
    let out = curl_provider_endpoint(&url, &args)?;
    let v: Value = serde_json::from_str(&out)
        .map_err(|_| format!("模型清单响应不是 JSON：{}", snippet(&out, 200)))?;
    if let Some(err) = v.get("error").filter(|e| !e.is_null()) {
        return Err(api_error(err));
    }
    // 标准结构 { "data": [ { "id": "..." }, ... ] }
    let arr = v
        .get("data")
        .and_then(|d| d.as_array())
        .ok_or_else(|| format!("模型清单缺少 data 字段：{}", snippet(&out, 200)))?;
    let mut ids: Vec<String> = arr
        .iter()
        .filter_map(|m| m.get("id").and_then(|x| x.as_str()).map(|s| s.trim().to_string()))
        .filter(|s| !s.is_empty())
        .collect();
    ids.sort();
    ids.dedup();
    if ids.is_empty() {
        return Err("上游返回的模型清单为空".into());
    }
    Ok(ids)
}

/// 经 curl 发 JSON POST。body 落临时文件（绕开命令行引号地狱 + 中文编码问题）。
fn curl_post_json(url: &str, headers: &[String], body: &Value) -> Result<Value, String> {
    curl_post_json_timeout(url, headers, body, 60)
}

/// 首启激活内置指纹 Key —— 「插上 U 盘就能用」的关键一步。
///
/// 把本地算出的指纹 key 发给 uclaw-pay 的 `/recharge/activate`，服务端按这把 key **mint
/// 一个绑定它的 new-api token**，并按 `activate.json` 的 `bonusCNY` 种入赠送额度（当前
/// ¥0.1 = 5 万 token）。**不走任何真实支付流水**。幂等：key 已存在则走服务端 dedup 分支，
/// 不重复 mint。防刷在服务端（每 IP 5 次/天 + 24h 滑窗，`activate_limiter.go`）。
///
/// 为什么必须有这步：`device.rs` 只在本地算指纹 key，服务端原本并不存在这把 token ——
/// 不激活直接拿去调模型会 `Invalid token`（实测）。激活后这把指纹 key 才真正可用。
///
/// 国内优先 `.cn`（裸网可达），失败退新加坡 `api.u-claw.org`。secret 防外部裸刷端点。
///
/// 返回：`Ok(_)` = 服务端已受理（HTTP 200，token 已 mint 或已存在 / 赠送被关），调用方据此
/// 落「已激活」标记不再重试；`Err` = 网络失败 / 限流(429) / 5xx，调用方下次重试。
/// **best-effort：绝不阻塞、失败也不影响本地 key 的写入与手动充值。**
pub fn activate_key(key: &str) -> Result<bool, String> {
    // 🔴 **不再有硬编码默认值。** 这里以前直接写死一个共享口令 —— 任何人都能拿任意
    // 随机串当「指纹」批量领赠送额度，唯一挡着的只有服务端每 IP 的限流。
    //
    // ⚠️ 那个口令**早已不是秘密**：它编进了每一个已发布的 exe，一条 grep 就抠得出来
    // （2026-08-18 在 `website/download/U-King.exe` 上实测命中）。所以从源码删掉它
    // **不等于收回它** —— 真正的处置是服务端把赠送额度归零，让这个口令一文不值。
    // 正因如此，这里连注释都不再复述那个字面量：仓库要公开了，少一处是一处。
    //
    // 现在凭证走设备钱包（`device.rs` 的 /device/bind），这条老路只在服务端还没部署
    // /device/* 时当兜底。官方构建可以通过 env 注入口令保住那个过渡窗口；开源构建里
    // 它是空的，于是整段直接跳过 —— **少一次调用，好过泄一个口令**。
    let secret = std::env::var("UKING_ACTIVATE_SECRET").unwrap_or_default();
    if secret.trim().is_empty() {
        return Err("未配置激活口令，跳过老激活链路".into());
    }
    let body = json!({ "apiKey": key, "secret": secret });
    let mut last = String::new();
    for host in ["https://api.u-claw.org.cn", "https://api.u-claw.org"] {
        let url = format!("{host}/recharge/activate");
        match curl_post_json(&url, &[], &body) {
            Ok(v) => {
                // 有 error 字段（如 rate-limited / forbidden）→ 当失败，换 host 或下次重试。
                if v.get("error").is_some() {
                    last = v
                        .get("message")
                        .or_else(|| v.get("error"))
                        .and_then(|x| x.as_str())
                        .unwrap_or("activate 被拒")
                        .to_string();
                    continue;
                }
                // 无 error 且带 activated 字段 → 已受理（true=新 mint，false=已存在/赠送关）。
                if let Some(b) = v.get("activated").and_then(|x| x.as_bool()) {
                    return Ok(b);
                }
                last = format!("响应异常：{}", snippet(&v.to_string(), 200));
            }
            Err(e) => last = e,
        }
    }
    Err(format!("激活失败：{last}"))
}

// ---------------------------------------------------------------------------
// 设备钱包（服务端 /device/* 端点）
// ---------------------------------------------------------------------------

/// 服务端签发的一把访问凭证。
#[derive(Debug, Clone)]
pub struct DeviceKeyIssue {
    pub wallet_id: String,
    /// 服务端生成的随机 key。**只在签发那一次拿得到**，必须立刻落盘。
    pub api_key: String,
    pub balance_tokens: i64,
}

/// 迁移第一步的三种结局。**必须分开处理** —— 三种都「没拿到新 key」，
/// 但该做的事完全不同：重试 / 当新机器绑定 / 提示凭订单号找回。
/// 混成一个 Err(String) 的话，调用方只能靠 match 错误文案，那种代码活不过一次改文案。
#[derive(Debug, Clone)]
pub enum MigrateOutcome {
    /// 200：新凭证已 mint，指纹 key 此刻仍然有效，等 commit。
    Staged(DeviceKeyIssue),
    /// 404：服务端不认识这把指纹 key —— 这台机器从没激活过，按新机器绑定。
    NotOurCustomer,
    /// 409：这台机器迁过了，但本地配置没了。指纹 key 已永久作废，
    /// 服务端不会（也不能）再把凭证发出来 —— 再发就等于凭空造一条通往钱包的路。
    AlreadyMigrated,
    /// 服务端还没部署 /device/* —— 客户端比服务端先发出去了。退回老的指纹+激活路径，
    /// 别把产品搞成「新版一装上就没 key」。
    NotDeployed,
}

/// 这个 404 是「路由不存在」还是「业务上没找到」？
///
/// 两者都得处理，且处置相反：路由不存在 = 服务端还没升级，退回老路；
/// 业务没找到 = 服务端认得这个接口，只是不认识这把 key。
///
/// 判据是 body 里有没有我们自己写的 `error` 字段 —— gin 的默认 404 是纯文本
/// `404 page not found`，解析成 JSON 会得到空对象。
fn is_route_missing(status: u16, v: &Value) -> bool {
    status == 404 && v.get("error").is_none()
}

/// 设备钱包服务的两个入口。国内优先 `.cn`（裸网可达），失败退新加坡。
/// 和 `activate_key` 同一份顺序 —— 这两个域的可达性差异见 CLAUDE.md「国内可达性铁律」。
const DEVICE_HOSTS: [&str; 2] = ["https://api.u-claw.org.cn", "https://api.u-claw.org"];

/// 新装机：让服务端生成一把随机凭证并建钱包。
/// 服务端还没部署 /device/* 时返回的错误文案。调用方按它退回老路 ——
/// 用常量而不是散落的字符串，是因为「靠 match 错误文案」的代码活不过一次改文案。
pub const DEVICE_API_NOT_DEPLOYED: &str = "device-api-not-deployed";

pub fn device_bind(hw_hint: &str, platform: &str, channel: &str) -> Result<DeviceKeyIssue, String> {
    let body = json!({ "hwHint": hw_hint, "platform": platform, "channel": channel });
    let (status, v) = device_post("/device/bind", &body)?;
    if is_route_missing(status, &v) {
        return Err(DEVICE_API_NOT_DEPLOYED.into());
    }
    parse_key_issue(&v)
}

/// 轮换第一步：mint 一把新凭证。**旧凭证此刻仍然有效**，别急着删配置。
pub fn device_rotate(current_key: &str) -> Result<DeviceKeyIssue, String> {
    let body = json!({ "currentKey": current_key });
    let (_, v) = device_post("/device/rotate", &body)?;
    parse_key_issue(&v)
}

/// 轮换第二步：搬余额、吊销旧凭证。调用前必须已经把新 key 写进各 CLI 配置并验通。
pub fn device_rotate_commit(current_key: &str, new_key: &str) -> Result<i64, String> {
    let body = json!({ "currentKey": current_key, "newKey": new_key });
    let (_, v) = device_post("/device/rotate/commit", &body)?;
    Ok(v.get("balanceTokens").and_then(|x| x.as_i64()).unwrap_or(0))
}

/// 老客户迁移第一步：拿指纹 key 换一把随机凭证。
pub fn device_migrate(
    fingerprint_key: &str,
    hw_hint: &str,
    platform: &str,
) -> Result<MigrateOutcome, String> {
    let body = json!({
        "fingerprintKey": fingerprint_key,
        "hwHint": hw_hint,
        "platform": platform,
    });
    let (status, v) = device_post("/device/migrate", &body)?;
    if is_route_missing(status, &v) {
        return Ok(MigrateOutcome::NotDeployed);
    }
    match status {
        404 => Ok(MigrateOutcome::NotOurCustomer),
        409 => Ok(MigrateOutcome::AlreadyMigrated),
        200 => Ok(MigrateOutcome::Staged(parse_key_issue(&v)?)),
        other => Err(format!(
            "迁移失败（HTTP {other}）：{}",
            v.get("message")
                .or_else(|| v.get("error"))
                .and_then(|x| x.as_str())
                .unwrap_or("未知错误")
        )),
    }
}

/// 老客户迁移第二步：搬余额、**永久吊销指纹 key**。
pub fn device_migrate_commit(fingerprint_key: &str, new_key: &str) -> Result<i64, String> {
    let body = json!({ "fingerprintKey": fingerprint_key, "newKey": new_key });
    let (_, v) = device_post("/device/migrate/commit", &body)?;
    Ok(v.get("balanceTokens").and_then(|x| x.as_i64()).unwrap_or(0))
}

/// 打设备钱包服务的设备端点，两个 host 依次试。
///
/// 返回 `(HTTP 状态码, 响应 JSON)`。**状态码不能丢** —— migrate 的 404 和 409
/// 是两个完全不同的处置（当新机器绑 / 提示凭订单号找回），只看 body 分不出来。
///
/// 4xx 不换 host 重试：那是服务端明确的判决，换个域名再问一遍只会得到同一个答案，
/// 白白多一次超时。只有网络失败和 5xx 才退到第二个 host。
fn device_post(path: &str, body: &Value) -> Result<(u16, Value), String> {
    let mut last = String::new();
    let mut maybe_not_deployed = false;
    for host in DEVICE_HOSTS {
        let url = format!("{host}{path}");
        match curl_post_json_status(&url, &[], body) {
            Ok((status, v)) => {
                if (200..300).contains(&status) {
                    // 2xx 但 body 是空对象 → 这 host 把请求吞进了某个兜底页（new-api
                    // 首页 / HTML 解析失败），**不是**设备钱包服务的响应。别把它当
                    // 「迁移成功」去解析 apiKey —— 那是把「没部署」报成「已在搬余额」。
                    // 换下一个 host 再试；全部如此才算整条 /device 未部署。
                    if v.is_null() || v.as_object().map(|o| o.is_empty()).unwrap_or(true) {
                        maybe_not_deployed = true;
                        last = format!("HTTP {status}（空响应，疑似路由未部署）");
                        continue;
                    }
                    return Ok((status, v));
                }
                if (400..500).contains(&status) {
                    return Ok((status, v));
                }
                last = format!("HTTP {status}");
            }
            Err(e) => last = e,
        }
    }
    // 都给了「2xx 空响应」→ 整条 /device 不在任何 host 上，明确退回老路（reconcile 认这个常量）。
    if maybe_not_deployed {
        return Err(DEVICE_API_NOT_DEPLOYED.to_string());
    }
    Err(format!("{path} 请求失败：{last}"))
}

fn parse_key_issue(v: &Value) -> Result<DeviceKeyIssue, String> {
    let api_key = v
        .get("apiKey")
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("响应缺少 apiKey：{}", snippet(&v.to_string(), 200)))?;
    Ok(DeviceKeyIssue {
        wallet_id: v
            .get("walletId")
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .to_string(),
        api_key: api_key.to_string(),
        balance_tokens: v.get("balanceTokens").and_then(|x| x.as_i64()).unwrap_or(0),
    })
}

/// POST JSON 并把 HTTP 状态码一起带回来。
///
/// 用 `-w '\n%{http_code}'` 把状态码追在响应体后面，再从**最后一个换行**切开 ——
/// 不能用 `split('\n').last()` 之外的切法：JSON 体里带换行是常态，从前面切会把
/// 响应体截断成半截 JSON，然后报一个「响应不是 JSON」的假故障。
fn curl_post_json_status(
    url: &str,
    headers: &[String],
    body: &Value,
) -> Result<(u16, Value), String> {
    let tmp = std::env::temp_dir().join(format!("uking-devreq-{}.json", std::process::id()));
    std::fs::write(&tmp, serde_json::to_vec(body).unwrap())
        .map_err(|e| format!("写请求临时文件失败: {e}"))?;
    let data = format!("@{}", tmp.display());

    let mut args: Vec<&str> = vec!["-sS", "-m", "20", "-X", "POST", url, "-w", "\n%{http_code}"];
    for h in headers {
        args.push("-H");
        args.push(h);
    }
    args.extend(["-H", "Content-Type: application/json", "--data", &data]);

    let out = curl(&args);
    let _ = std::fs::remove_file(&tmp);
    let out = out?;

    let (raw_body, raw_status) = out
        .rsplit_once('\n')
        .ok_or_else(|| format!("响应格式异常：{}", snippet(&out, 200)))?;
    let status: u16 = raw_status
        .trim()
        .parse()
        .map_err(|_| format!("状态码解析失败：{}", snippet(raw_status, 40)))?;
    // 4xx/5xx 的响应体可能是 nginx 的 HTML 而不是 JSON。那种情况不该整个失败 ——
    // 状态码本身就是我们要的信息，body 给个空对象即可。
    let v: Value = serde_json::from_str(raw_body.trim()).unwrap_or_else(|_| json!({}));
    Ok((status, v))
}

/// 带超时的 POST JSON。作图返回 1~2MB base64 + 模型出图本身慢，60s 不够（实测客户机
/// curl 退出码 28：60s 内只收到 286KB/1.7MB → 超时）。作图链路用 180s。
fn curl_post_json_timeout(
    url: &str,
    headers: &[String],
    body: &Value,
    timeout_s: u32,
) -> Result<Value, String> {
    let tmp = std::env::temp_dir().join(format!(
        "uking-req-{}.json",
        std::process::id()
    ));
    std::fs::write(&tmp, serde_json::to_vec(body).unwrap())
        .map_err(|e| format!("写请求临时文件失败: {e}"))?;
    let data = format!("@{}", tmp.display());
    let tmo = timeout_s.to_string();

    let mut args: Vec<&str> = vec!["-sS", "-m", &tmo, "-X", "POST", url];
    for h in headers {
        args.push("-H");
        args.push(h);
    }
    args.extend(["-H", "Content-Type: application/json", "--data", &data]);

    let out = curl_provider_endpoint(url, &args);
    let _ = std::fs::remove_file(&tmp);
    let out = out?;
    serde_json::from_str(&out).map_err(|_| format!("响应不是 JSON：{}", snippet(&out, 300)))
}

/// 调外部供应商时，GUI 双击启动没有终端继承的 `HTTPS_PROXY`，但 Windows 系统代理仍是
/// 用户已明确配置的出网路径。非虾盘云端点（例如海外的 OpenAI 兼容中转）在这里补上它；
/// 虾盘云坚持国内直连，避免被失效代理拖死。
///
/// 已由调用者显式设置代理环境时不覆盖：显式环境的优先级高于 Windows 注册表设置。
fn curl_provider_endpoint(url: &str, args: &[&str]) -> Result<String, String> {
    if is_xiapan_endpoint(url)
        || std::env::var_os("HTTPS_PROXY").is_some()
        || std::env::var_os("https_proxy").is_some()
        || std::env::var_os("HTTP_PROXY").is_some()
        || std::env::var_os("http_proxy").is_some()
    {
        return curl(args);
    }

    let proxy = crate::installer::system_proxy_env()
        .into_iter()
        .find_map(|(name, value)| (name == "HTTPS_PROXY").then_some(value));
    let Some(proxy) = proxy else {
        return curl(args);
    };

    let mut full: Vec<&str> = Vec::with_capacity(args.len() + 2);
    full.extend(["--proxy", proxy.as_str()]);
    full.extend_from_slice(args);
    curl(&full)
}

fn openai_chat(base: &str, key: &str, model: &str, prompt: &str) -> Result<String, String> {
    openai_chat_full(base, key, model, None, prompt, 128)
}

/// OpenAI Responses 格式实测（新版 Codex 唯一认的协议）。
/// 不限 max_output_tokens：codex 系是推理模型，预算太小会在思考阶段被截断拿不到正文。
fn openai_responses(base: &str, key: &str, model: &str, prompt: &str) -> Result<String, String> {
    let url = format!("{}/responses", base.trim_end_matches('/'));
    let body = json!({
        "model": model,
        "input": [{"role": "user", "content": [{"type": "input_text", "text": prompt}]}],
        "stream": false
    });
    let v = curl_post_json(&url, &[format!("Authorization: Bearer {key}")], &body)?;
    if let Some(err) = v.get("error").filter(|e| !e.is_null()) {
        return Err(api_error(err));
    }
    // output 是块数组：找 type=="message" 里 type=="output_text" 的文本
    v.get("output")
        .and_then(|o| o.as_array())
        .and_then(|arr| {
            arr.iter()
                .filter(|item| item.get("type").and_then(|t| t.as_str()) == Some("message"))
                .find_map(|item| {
                    item.get("content").and_then(|c| c.as_array()).and_then(|blocks| {
                        blocks.iter().find_map(|b| {
                            if b.get("type").and_then(|t| t.as_str()) == Some("output_text") {
                                b.get("text").and_then(|x| x.as_str()).map(|s| s.trim().to_string())
                            } else {
                                None
                            }
                        })
                    })
                })
        })
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("Responses 响应缺少正文：{}", snippet(&v.to_string(), 200)))
}

fn openai_chat_full(
    base: &str,
    key: &str,
    model: &str,
    system: Option<&str>,
    prompt: &str,
    max_tokens: u32,
) -> Result<String, String> {
    let url = format!("{}/chat/completions", base.trim_end_matches('/'));
    let mut messages = Vec::new();
    if let Some(s) = system {
        messages.push(json!({"role": "system", "content": s}));
    }
    messages.push(json!({"role": "user", "content": prompt}));
    let body = json!({
        "model": model,
        "max_tokens": max_tokens,
        "messages": messages
    });
    let v = curl_post_json(&url, &[format!("Authorization: Bearer {key}")], &body)?;
    if let Some(err) = v.get("error") {
        return Err(api_error(err));
    }
    let msg = v.pointer("/choices/0/message");
    // content 为主；推理型模型（如 deepseek-v4-flash）有时把全部输出塞进
    // reasoning_content 而 content 为空 —— 回退读它，别丢内容（修 ai_diagnose #7）。
    let content = msg
        .and_then(|m| m.get("content"))
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let text = content
        .or_else(|| {
            msg.and_then(|m| m.get("reasoning_content"))
                .and_then(|x| x.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
        })
        .map(|s| s.to_string());
    text.ok_or_else(|| format!("响应缺少回复内容：{}", snippet(&v.to_string(), 200)))
}

// ============================================================
// AI 修复（API 直连大脑 —— 不依赖 claude CLI，修的就是装不上 claude 的问题）
// ============================================================

#[derive(Debug, Clone, Serialize)]
pub struct Diagnosis {
    pub diagnosis: String,
    pub commands: Vec<String>,
}

const REPAIR_SYSTEM_PROMPT: &str = r#"你是 Windows 装机修复专家，正在帮一个图形界面管家（U-King）修复 AI 编程工具（Claude Code / Codex CLI，经 npm 安装）的安装问题。
用户会给你：失败的工具名 + 安装日志尾部 + 环境体检结果。
你必须只返回一个严格 JSON 对象（不要 markdown 代码块，不要多余文字）：
{"diagnosis":"一两句中文说明问题根因","commands":["修复命令1","修复命令2"]}
要求：
- commands 是 Windows cmd 命令，最多 4 条，按执行顺序排列；修完后管家会自动重装验证，所以不要包含重装命令本身
- 只做与本次安装修复直接相关的安全操作（清缓存、改 npm 配置、删坏目录、设代理/源等）
- 国内网络环境，npm 源优先 https://registry.npmmirror.com
- 禁止：格式化/分区/关机/改系统关键注册表/删除用户数据
- 如果问题无法用命令修复（如磁盘满、需要人工操作），commands 给空数组，在 diagnosis 里说明让用户怎么做"#;

/// 错误是不是「这把 Key 没权限用这个模型」—— 只有这类错换模型才有救。
/// 网络错 / 超时 / 401（key 本身无效）换模型救不了，直接抛原错误别白烧一跳。
fn is_model_access_error(err: &str) -> bool {
    let e = err.to_lowercase();
    e.contains("no access to model")
        || e.contains("not have access")
        || e.contains("model_not_found")
        || e.contains("does not exist")
        || e.contains("无权")
        || e.contains("无可用渠道")
}

/// 让虾盘云上的模型诊断安装失败，返回诊断 + 待执行修复命令（由用户确认后执行）。
pub fn ai_diagnose(api_key: &str, context: &str) -> Result<Diagnosis, String> {
    // 首选 gpt-5.4-mini：实测把 JSON 干净放进 content（非推理型，无 reasoning_content）。
    // 但设备钱包签发的 Key 不一定开通它 —— pc-*** 实测报
    // 「This token has no access to model gpt-5.4-mini」，硬编码单模型 = 这类客户诊断全废。
    // 命中权限错时降级 deepseek-v4-flash（设备 Key 的基础盘，人人都有）；
    // 它是推理型，思考阶段就要吃预算，700 会在正文出来前被截断，所以提到 2000。
    // reasoning_content 回退 openai_chat_full 里已有，不用新解析路径。
    let base = "https://api.u-claw.org.cn/v1";
    let reply = match openai_chat_full(
        base,
        api_key,
        "gpt-5.4-mini",
        Some(REPAIR_SYSTEM_PROMPT),
        context,
        700,
    ) {
        Ok(r) => r,
        Err(e1) if is_model_access_error(&e1) => openai_chat_full(
            base,
            api_key,
            "deepseek-v4-flash",
            Some(REPAIR_SYSTEM_PROMPT),
            context,
            2000,
        )
        .map_err(|e2| format!("gpt-5.4-mini: {e1}；降级 deepseek-v4-flash: {e2}"))?,
        Err(e1) => return Err(e1),
    };

    // 宽松提取第一个完整 {...}（模型偶尔包 markdown / 前缀说明）
    if let (Some(start), Some(end)) = (reply.find('{'), reply.rfind('}')) {
        if end > start {
            if let Ok(v) = serde_json::from_str::<Value>(&reply[start..=end]) {
                let diagnosis = v
                    .get("diagnosis")
                    .and_then(|x| x.as_str())
                    .unwrap_or("（AI 未给出诊断说明）")
                    .to_string();
                let commands = v
                    .get("commands")
                    .and_then(|x| x.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|c| c.as_str())
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .take(4)
                            .collect()
                    })
                    .unwrap_or_default();
                return Ok(Diagnosis { diagnosis, commands });
            }
        }
    }

    // 兜底：模型没给 JSON（纯文本分析）也别整个失败——把文本当诊断展示，不给自动命令。
    // 用户至少能看到 AI 的判断，自己决定怎么办，比报「诊断失败」强。
    let text = reply.trim();
    if text.is_empty() {
        return Err("AI 未返回任何内容，请稍后重试".into());
    }
    Ok(Diagnosis {
        diagnosis: snippet(text, 600),
        commands: Vec::new(),
    })
}

fn anthropic_chat(base: &str, key: &str, model: &str, prompt: &str) -> Result<String, String> {
    let url = format!("{}/v1/messages", base.trim_end_matches('/'));
    let body = json!({
        "model": model,
        "max_tokens": 128,
        "messages": [{"role": "user", "content": prompt}]
    });
    let v = curl_post_json(
        &url,
        &[
            format!("x-api-key: {key}"),
            "anthropic-version: 2023-06-01".to_string(),
        ],
        &body,
    )?;
    if let Some(err) = v.get("error") {
        return Err(api_error(err));
    }
    // content 是块数组；推理型模型（如 deepseek-v4-pro）会先回 thinking/signature 块，
    // 取第一个 type=="text" 的块才稳。
    v.get("content")
        .and_then(|c| c.as_array())
        .and_then(|arr| {
            arr.iter().find_map(|b| {
                if b.get("type").and_then(|t| t.as_str()) == Some("text") {
                    b.get("text").and_then(|x| x.as_str()).map(|s| s.trim().to_string())
                } else {
                    None
                }
            })
        })
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("响应缺少回复内容：{}", snippet(&v.to_string(), 200)))
}

fn api_error(err: &Value) -> String {
    err.get("message")
        .and_then(|m| m.as_str())
        .map(String::from)
        .unwrap_or_else(|| snippet(&err.to_string(), 200))
}

fn snippet(s: &str, n: usize) -> String {
    let t = s.trim();
    if t.len() <= n {
        t.into()
    } else {
        // 按字符边界截断，避免切坏 UTF-8
        let cut: String = t.chars().take(n).collect();
        format!("{cut}…")
    }
}

// ============================================================
// AI 作图（虾盘云 /v1/images/generations）
// ============================================================

#[derive(Debug, Clone, Serialize)]
pub struct ImageResult {
    /// base64 PNG（response_format=b64_json 时）；前端拼 data:image/png;base64,
    pub b64: Option<String>,
    /// 图片 URL（上游不支持 b64 时回退）
    pub url: Option<String>,
    pub model: String,
    /// 上游改写后的实际 prompt（DALL·E 系会返回）
    pub revised_prompt: Option<String>,
    /// 若本图是「原模型被安全系统拒绝后自动换模型重画」出来的，这里记原模型的友好名
    /// （如 "GPT Image 2"）。前端据此提示客户「已自动换 Seedream 重画」。正常出图为 None。
    pub fallback_from: Option<String>,
}

/// 作图端点（文生图 = generations，图生图 = edits）。国内裸网可达的 .cn 域名。
/// **这两个是「默认」不是「唯一」** —— 客户在 AI 设置里选了别家就走 [`draw_endpoint`]。
const IMAGE_GEN_URL: &str = "https://api.u-claw.org.cn/v1/images/generations";
const IMAGE_EDIT_URL: &str = "https://api.u-claw.org.cn/v1/images/edits";

/// 内置作图供应商的 id（= 默认路径）。
const XIAPAN_ID: &str = "xiapan";

// ============================================================
// 作图路由（~/.uking/draw-route.json）
// ============================================================
//
// 「作图走哪家」是**一笔记录**，不是一次 apply。`apply_provider` 是往外部工具（Claude Code /
// Codex / ClawX / Hermes）的配置文件里写字的机器 —— 那些工具有自己的进程、自己的配置格式，
// 所以"应用"才需要真去改文件。作图是 U-King 自己的内部能力，它的"应用"只是记下用户选了谁，
// 下次出图时读一遍。给它套 apply_provider 那套（备份、回读校验、重启进程）是拿马车的规矩管走路。
//
// 🔴 **为什么不塞进 `active-drivers.json`**：那份表的值是**纯字符串** provider id
// （`driver_status` / `list_providers_for` 都按这个形状解析），作图要记的是「provider + 模型」
// 两件事。往一份被 8 个工具共用的表里塞对象形状，等于让所有读它的地方都得先判类型 ——
// 同一事实存在几份就会漂移几份（宪法第 8 条），而这里连"几份"都不用，单开一个文件最便宜。

/// 磁盘上那笔记录。**只有用户在 AI 设置里点过「应用」才存在**；不存在 = 走内置虾盘云。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DrawRoute {
    #[serde(default)]
    pub provider_id: String,
    /// 空 = 不覆盖，听「AI 作图」页那个模型下拉的。
    #[serde(default)]
    pub model: String,
}

/// 解析结果 —— **唯一**一处回答「这次出图打谁的端点、用谁的 Key、用哪个模型」。
#[derive(Debug, Clone)]
pub struct DrawEndpoint {
    pub provider_id: String,
    pub provider_name: String,
    pub gen_url: String,
    pub edit_url: String,
    /// `None` = 用调用方传进来的设备钱包 Key（只有默认的虾盘云路径才是 None）。
    /// `Some(k)` = 用这家自己的 Key。🔴 **绝不拿设备钱包的 Key 去打别人的端点** ——
    /// 那把 Key 是我们签发的、余额记在我们账上，发给第三方等于把客户的钱包交出去。
    pub api_key: Option<String>,
    /// `None` = 不覆盖调用方选的模型。
    pub model: Option<String>,
    /// 走的是内置默认路径（虾盘云）。兜底换模型、前端的模型下拉都只在这条路上成立。
    pub builtin: bool,
}

/// 给前端回显用的形状（**不含 api_key**：前端不需要它，少一处泄漏面少一次事故）。
#[derive(Debug, Clone, Serialize)]
pub struct DrawRouteView {
    pub provider_id: String,
    pub provider_name: String,
    pub model: String,
    pub builtin: bool,
    /// 回显「到底打谁」——客户报「作图不出图」时，这一行比任何截图都省事。
    pub gen_url: String,
}

fn draw_route_path() -> PathBuf {
    config_home().join(".uking").join("draw-route.json")
}

/// 读那笔记录。文件不存在 / 解析不了一律回默认（= 走虾盘云），**绝不让作图因为一个坏 json 哑掉**。
pub fn read_draw_route() -> DrawRoute {
    std::fs::read_to_string(draw_route_path())
        .ok()
        .and_then(|s| serde_json::from_str::<DrawRoute>(&s).ok())
        .unwrap_or_default()
}

/// 记一笔「作图走谁」。存前校验 provider 真存在且有 OpenAI 兼容地址 ——
/// 作图打的是 `/v1/images/*`，没有 `openai_base` 就拼不出端点，让它存下去只会把失败
/// 推迟到客户点「生成」的那一刻（那时报错离原因已经隔了三层）。
pub fn set_draw_route(provider_id: &str, model: &str) -> Result<DrawRoute, String> {
    let id = provider_id.trim();
    if id.is_empty() {
        return Err("请先选一家供应商".into());
    }
    let p = all_providers()
        .into_iter()
        .find(|p| p.id == id)
        .ok_or_else(|| format!("没找到供应商「{id}」"))?;
    if p.openai_base.trim().is_empty() {
        return Err(format!(
            "「{}」没填 OpenAI 兼容地址。作图走的是 /v1/images/generations，必须有它。",
            p.name
        ));
    }
    // 🔴 非默认路径**必须**指定模型。留空看着像"用作图页选的那个"，可作图页那个下拉列的是
    // 虾盘云的模型 id（gpt-image-2 / seedream-4-0 …），拿去打别人的端点基本必 404 ——
    // 而且客户端一切正常、报错来自上游，客户只会归因成"这功能坏了"。
    // 一个含义是谎话的选项，不如不给。虾盘云那条不受此限：它的模型真相源就是作图页。
    if id != XIAPAN_ID && model.trim().is_empty() {
        return Err(format!(
            "请填「{}」的作图模型 id（如 flux-pro / dall-e-3）。留空会拿虾盘云的模型名去打它的端点，必然报错。",
            p.name
        ));
    }
    let route = DrawRoute {
        provider_id: id.to_string(),
        model: model.trim().to_string(),
    };
    let body = serde_json::to_vec_pretty(&route).map_err(|e| format!("序列化失败: {e}"))?;
    atomic_write(&draw_route_path(), &body)?;
    Ok(route)
}

/// `{base}` → `{base}/images/{kind}`。规则抄 `agent::chat::chat_endpoint`（同一套拼法只该有一种）：
/// 尾斜杠先削（否则拼出 `//images/generations`），客户把整条路径填进 base 了就先削回 base ——
/// 不削的话给 edits 会拼出 `…/images/generations/images/edits`，而这种填法实测真有人用。
fn images_endpoint(base: &str, kind: &str) -> String {
    let mut b = base.trim().trim_end_matches('/');
    for full in ["/images/generations", "/images/edits"] {
        if let Some(stripped) = b.strip_suffix(full) {
            b = stripped;
            break;
        }
    }
    format!("{}/images/{kind}", b.trim_end_matches('/'))
}

impl DrawEndpoint {
    /// 内置默认路径。**这条必须与「这功能还没被改过」时逐字节一致**：常量 URL、
    /// Key 用调用方传进来的设备钱包 Key、模型听作图页那个下拉的。
    fn builtin() -> Self {
        Self {
            provider_id: XIAPAN_ID.into(),
            provider_name: builtin_providers()
                .into_iter()
                .find(|p| p.id == XIAPAN_ID)
                .map(|p| p.name)
                .unwrap_or_else(|| "虾盘云".into()),
            gen_url: IMAGE_GEN_URL.into(),
            edit_url: IMAGE_EDIT_URL.into(),
            api_key: None,
            model: None,
            builtin: true,
        }
    }

    /// 这次真正该用的 Key。路由带 Key 就用路由的，只有默认路径才回落设备钱包 Key。
    fn effective_key(&self, device_key: &str) -> Result<String, String> {
        match self.api_key.as_deref() {
            Some(k) if !k.trim().is_empty() => Ok(k.to_string()),
            // 选了别家却没填 Key：说清是哪家、去哪补，别复用那句"请先充值开通虾盘云"——
            // 客户明明选的是自己的供应商，被劝去给我们充值只会更懵。
            Some(_) => Err(format!(
                "「{}」还没填 API Key。请到「AI 设置 → 供应商库」补上它的 Key 再出图。",
                self.provider_name
            )),
            None if device_key.trim().is_empty() => Err("缺少 API Key（请先充值开通虾盘云）".into()),
            None => Ok(device_key.to_string()),
        }
    }

    /// 这次真正该用的模型。默认路径不覆盖 —— 作图页那个下拉是离用户最近的控件，
    /// 不许被一个藏在设置里的默认值悄悄压过去（两个真相源必然漂移）。
    fn effective_model(&self, requested: &str) -> String {
        match self.model.as_deref() {
            Some(m) if !m.trim().is_empty() => m.trim().to_string(),
            _ => requested.to_string(),
        }
    }
}

/// 解析作图路由。**所有出图路径都从这里问一次**（GUI / 影核动作 / 小程序 / agent 工具）。
pub fn draw_endpoint() -> DrawEndpoint {
    let r = read_draw_route();
    let id = r.provider_id.trim();
    if id.is_empty() || id == XIAPAN_ID {
        return DrawEndpoint::builtin();
    }
    // 记录指向一个已经被删掉的供应商 → 回落默认，别让作图整个哑掉。
    // 删供应商那条路不知道作图引用了它；靠"读的时候发现没了"兜住，比在删除处反向加一堆
    // 依赖便宜得多（而且真删的那天才发现耦合，是这个项目栽过的跟头）。
    let Some(p) = all_providers().into_iter().find(|p| p.id == id) else {
        return DrawEndpoint::builtin();
    };
    let base = p.openai_base.trim();
    if base.is_empty() {
        return DrawEndpoint::builtin();
    }
    DrawEndpoint {
        gen_url: images_endpoint(base, "generations"),
        edit_url: images_endpoint(base, "edits"),
        api_key: Some(p.api_key.clone()),
        model: Some(r.model.trim().to_string()).filter(|m| !m.is_empty()),
        provider_id: p.id,
        provider_name: p.name,
        builtin: false,
    }
}

/// 回显给前端（AI 设置那张卡 + 作图页顶部 banner）。
pub fn draw_route_view() -> DrawRouteView {
    let e = draw_endpoint();
    DrawRouteView {
        provider_id: e.provider_id,
        provider_name: e.provider_name,
        model: e.model.unwrap_or_default(),
        builtin: e.builtin,
        gen_url: e.gen_url,
    }
}

/// 作图链路超时（秒）。作图是「同步出图」——服务器画完整张才一次性回 JSON，画的过程客户端
/// 收到 0 字节。文字 / 要素多的海报（如「南极洋三文鱼广告牌」：店名+中英广告语+LOGO+二维码+地址）
/// 上游要画 2~4 分钟，180s 实测仍 curl 28（0 bytes received）。2026-07-03 定「宁可等久点也别失败」
/// 放宽到 600s（10 分钟）；新加坡 nginx `u-claw-cn.conf` 的 `/v1/` 反代 `proxy_read_timeout` 已同步
/// 改到 600s（原先两边都卡在 300s，只改客户端超时无效——nginx 会先把连接掐掉）。
const IMAGE_GEN_TIMEOUT_S: u32 = 600;

/// 「安全拒绝」兜底模型：GPT Image（Azure）拒了真人 / 敏感照片编辑时换它重画。
/// 选 seedream-4-0（字节）——挑的是**审核宽松**：对真实照片编辑不挑剔、中文听得懂、
/// `/v1/images/edits` 端点 1024×1024 直接可用（seedream-4-5 要 ≥3.6M 像素尺寸，gemini 系不支持
/// edits 端点，故选 4-0）。这条看的是「换个不那么严的审核」，不是「换个更稳的供应商」。
const SAFETY_FALLBACK_EDIT_MODEL: &str = "seedream-4-0";

/// 「上游挂了」兜底模型 —— 挑的标准和上面**完全不同**：要的是**供应商独立**。
///
/// 2026-07-28 排查实锤（虾盘云 new-api abilities 路由表）：
///   · gpt-image-2   → 渠道 5  图像中转 A
///   · seedream-4-0  → 渠道 7  图像中转 A（**同一家**）
///   · qwen-image / qwen-image-edit-plus → 渠道 2 阿里云百炼（**国内直连，不过任何中转商**）
/// 所以上游故障时**绝不能**兜到 seedream —— 那是从一个中转商摔到同一个中转商，
/// 中转商整体限速/欠费时（这正是当天的故障形态）两个一起死。只有阿里直连是真正独立的第二条腿。
/// 两个端点分开配：qwen-image 只支持文生图，改图要用 qwen-image-edit-plus（两者均已对
/// IMAGE_SIZES 各档实测通过）。
const OUTAGE_FALLBACK_GEN_MODEL: &str = "qwen-image";
const OUTAGE_FALLBACK_EDIT_MODEL: &str = "qwen-image-edit-plus";

/// 是否为上游「内容安全系统拒绝」错误（GPT Image 的 Azure 安全过滤最典型：真人肖像 / 品牌 IP）。
/// 用于判断值不值得自动换个更宽松的模型重试一次。
fn is_safety_rejection(e: &str) -> bool {
    let low = e.to_lowercase();
    low.contains("safety system")
        || low.contains("rejected by the safety")
        || low.contains("content filter")
        || low.contains("content_policy")
        || low.contains("content policy")
}

/// 是否为「上游供应商自己挂了」——换个模型就能好，重试同一个模型没用。
///
/// 2026-07-28 实锤：gpt-image-2 的海外中转商出口 IP 被 Cloudflare 限速，全量返回
/// `429 {"upstream_message":"OpenAI was rate limited by Cloudflare (error code: 1015)"}`。
/// 服务端把它翻成「请求太频繁…避免连续快速点击生成按钮」，**归属判反**：客户没点快，
/// 等多久都不会好。这种情况下唯一有效的动作是换供应商，所以后端直接替他换一次。
///
/// 判定**故意收窄**，只认「换个供应商就能解决」的：
///   · 429 / rate limit / Cloudflare 1015 / 服务器繁忙 / 过载
///   · 5xx / bad gateway / 网关抖动
/// 明确**不认**（换模型无济于事，换了只会白烧一次钱）：
///   · 余额不足、Key 无效     —— 换哪个模型都一样没钱
///   · 安全拒绝               —— 走上面 `is_safety_rejection` 那条路
///   · 尺寸不合法             —— 参数错，换模型只会换个报错
///   · 超时                   —— 已经等了 10 分钟，再换一个模型等于让客户再等 10 分钟
fn is_upstream_outage(e: &str) -> bool {
    let low = e.to_lowercase();
    // 先排除「换模型也救不了」的，避免关键词误伤（如余额报错里恰好带 5xx 字样）
    if low.contains("quota")
        || low.contains("insufficient")
        || low.contains("invalid token")
        || low.contains("unauthorized")
        || e.contains("余额不足")
        || e.contains("额度不足")
        || is_safety_rejection(e)
        || low.contains("timed out")
        || low.contains("timeout")
        || e.contains("(28)")
        || e.contains("超时")
    {
        return false;
    }
    low.contains("rate limit")
        || low.contains("rate_limited")
        || low.contains("too many requests")
        || low.contains("error code: 1015")
        || low.contains("overloaded")
        || low.contains("bad gateway")
        || low.contains("service unavailable")
        || low.contains("\"429\"")
        || low.contains(" 429")
        || low.contains("(429)")
        || low.contains("502")
        || low.contains("503")
        || low.contains("504")
        || e.contains("太频繁")
        || e.contains("限流")
        || e.contains("繁忙")
}

/// 模型 id → 给客户看的友好名（`fallback_from` 用；前端据此提示「已自动换 X 重画」）。
fn friendly_image_model_name(id: &str) -> String {
    match id {
        "gpt-image-2" => "GPT Image 2".to_string(),
        "seedream-4-0" => "Seedream 4.0".to_string(),
        "qwen-image" | "qwen-image-edit-plus" => "通义千问图片".to_string(),
        other => other.to_string(),
    }
}

/// seedream 兜底时的尺寸：seedream-4-0 认「宽x高」像素值（实测 1024×1024 可用），不认 "auto" /
/// 比例文字。故用户尺寸是合法 WxH 就沿用（保留竖/横构图意图），否则退回 1024×1024。
fn seedream_edit_size(size: &str) -> String {
    let s = size.trim();
    let parts: Vec<&str> = s.split(|c| c == 'x' || c == 'X').collect();
    if parts.len() == 2
        && parts[0].trim().parse::<u32>().is_ok()
        && parts[1].trim().parse::<u32>().is_ok()
    {
        s.to_string()
    } else {
        "1024x1024".to_string()
    }
}

/// 把底层 curl 超时（退出码 28 / "timed out"）翻成小白能懂、且对「已经在用 gpt-image-2」也成立的
/// 话术（旧文案一律叫人「改用 GPT Image 2」，但人家正是用它超时的，自相矛盾）。其余错误原样透出。
///
/// 🔴 **钱和 Key 那两条必须跟着路由走**（2026-08-22 解绑虾盘云时补的）：客户把作图改到自己
/// 那家之后，钱压根不在虾盘云扣、Key 也不是我们发的 —— 再劝他「点右上角充值」是把人往
/// 反方向指。这个项目栽过同型的跟头（Mac 上念 Windows 的装机清单）：**平台/路由分支只做到
/// 后端、文案层没跟上，跑道全绿而客户看到的是错的**。
fn friendlier_image_error(e: String, route: &DrawEndpoint) -> String {
    let low = e.to_lowercase();
    // 余额不足（预扣失败）：翻成人话 + 引导充值（与视频侧一致，别把它当 bug）
    if low.contains("quota is not enough") || e.contains("余额不足") || low.contains("insufficient") {
        return if route.builtin {
            "余额不足：当前虾盘云余额可能不够这次出图。请点右上角「充值」补足后再试。".to_string()
        } else {
            format!(
                "「{}」那边余额/额度不够了。作图现在走的是这家，充值请去它自己的后台（不是虾盘云）。\
想换回内置的虾盘云，在「AI 设置 → 工具分配 → AI 作图」里改。",
                route.provider_name
            )
        };
    }
    if low.contains("invalid token") || low.contains("unauthorized") || low.contains("401") {
        return if route.builtin {
            "Key 暂时不可用：通常是这台电脑的虾盘云 Key 还没有充值开通，或余额已用完。请点右上角「充值」后再试。".to_string()
        } else {
            format!(
                "「{}」拒绝了这把 Key（无效 / 已过期 / 没开通作图权限）。请到「AI 设置 → 供应商库」\
核对它的 API Key —— 这不是虾盘云的 Key，右上角那个「充值」帮不上忙。",
                route.provider_name
            )
        };
    }
    if low.contains("safety system") || low.contains("policy") || low.contains("content filter") {
        return "图片安全系统拒绝了这个提示词。通常是因为包含真实人物姓名、具体影视/游戏 IP、敏感肖像或容易被误判的表达。请换个更中性的描述再试。".to_string();
    }
    if low.contains("invalid file or mode for image") || low.contains("invalid image format") {
        return "参考图格式或色彩模式不兼容。请在 U-King 里重新选择这张图（会自动转成 RGB/RGBA PNG），或先另存为 PNG/JPG 再试。".to_string();
    }
    // 尺寸参数不是合法的「宽x高」像素值（常见于客户/AI 把「3:4」这类比例文字直接填进尺寸框）。
    // 2026-07-03 生产日志实锤：channel #7 报 `size must be one of 'WIDTHxHEIGHT', '1k', '2k', or '4k'`。
    if low.contains("size") && (low.contains("not valid") || low.contains("must be one of") || low.contains("invalid size") || e.contains("尺寸不支持")) {
        return "尺寸格式不对：请用「宽x高」的像素值（如 1024x1536），不能直接填比例文字（如 3:4）。\
可以在尺寸下拉里选预设（已内置「3:4 竖图」），或按这条报错里给出的可用档位手填。"
            .to_string();
    }
    if e.contains("(28)") || low.contains("timed out") || low.contains("timeout") {
        return "出图超时了：这张图偏复杂（文字 / 要素越多，AI 画得越久），或当前网络较慢。\
建议 ① 把描述精简一点 ② 尺寸先用「1:1 方图」 ③ 过一两分钟再重试一次。\
（已把最长等待放宽到 10 分钟，仍超时多半是这一张太复杂）"
            .to_string();
    }
    // 网关/反代瞬时抖动：上游返回 502/503/504 的 HTML 错误页而非 JSON，之前直接把整段
    // <html>...openresty... 原文糊给用户看（issue #120 实锤）。识别出来翻成人话，别吓人。
    if low.contains("<html") && (low.contains("502") || low.contains("503") || low.contains("504")
        || low.contains("bad gateway") || low.contains("gateway time") || low.contains("openresty") || low.contains("nginx"))
    {
        return "服务器暂时繁忙（网关瞬时抖动），不是这张图的问题。请过几秒重试一次。".to_string();
    }
    e
}

/// 文生图：调虾盘云 generations 端点出图。复用系统 curl + api_error。
/// 先要 b64_json；上游若只回 url 则 `ensure_b64` 下载转 b64（保证落盘 + 离线可看）。
/// 画质档白名单：默认省钱 medium；老手可选 high（高清，约 4× 计费）。非法/空值一律回 medium，
/// 既省钱又防把乱值透给上游触发 400。只对 gpt-image 系生效（别的模型不认 quality）。
fn sanitize_quality(q: Option<&str>) -> &'static str {
    match q.map(|s| s.trim()) {
        Some("high") => "high",
        Some("low") => "low",
        Some("auto") => "auto",
        _ => "medium",
    }
}

/// 文生图跑一次（**不翻译**报错）—— 原始报错要留给 `is_upstream_outage` / `is_safety_rejection`
/// 判定用；翻译在最外层统一做（与 `generate_image_edit::edit_once` 同一约定）。
fn generate_once(
    gen_url: &str,
    api_key: &str,
    prompt: &str,
    model: &str,
    size: &str,
    quality: Option<&str>,
) -> Result<ImageResult, String> {
    let mut body = json!({
        "model": model,
        "prompt": prompt,
        "n": 1,
        "size": size,
        "response_format": "b64_json"
    });
    // 省钱默认 medium（标准图 ~¥0.35，肉眼几乎无差）；老手在「画质」选高清 → 传 high
    // （按图像 token 计费，单张 ¥1~3.6，约 4×）。只对 gpt-image* 加，其它模型不认 quality，多传会 400。
    if model.starts_with("gpt-image") {
        body["quality"] = json!(sanitize_quality(quality));
    }
    // 作图慢 + 回 1~2MB base64（IMAGE_GEN_TIMEOUT_S 说明见常量定义）。
    let v = curl_post_json_timeout(
        gen_url,
        &[format!("Authorization: Bearer {api_key}")],
        &body,
        IMAGE_GEN_TIMEOUT_S,
    )?;
    parse_image_response(&v, model)
}

pub fn generate_image(
    api_key: &str,
    prompt: &str,
    model: &str,
    size: &str,
    quality: Option<&str>,
) -> Result<ImageResult, String> {
    // 打谁的端点、用谁的 Key、用哪个模型 —— 一次问清，别在下面各分支各判一遍。
    let route = draw_endpoint();
    let api_key = &route.effective_key(api_key)?;
    if prompt.trim().is_empty() {
        return Err("请先输入要画的内容".into());
    }
    let model = &route.effective_model(model);
    let model = model.as_str();

    // 逐次落日志：作图是投诉最多的功能，而在此之前这条链路**一行日志都不写** ——
    // 客户说「出不了图」时，我们既不知道他用的哪个模型、也不知道兜底有没有触发、上游到底回了什么。
    // 只记供应商/模型/尺寸/提示词前 60 字/结果，**绝不记 api_key**（日志落客户本机，可能被转发）。
    // 供应商那一栏是解绑虾盘云之后加的：不记它，「出不了图」的第一个问题（打的谁？）就答不上来。
    let brief: String = prompt.chars().take(60).collect();
    crate::ulog::write(
        "draw",
        &format!(
            "出图请求 provider={} model={model} size={size} quality={} prompt={brief}",
            route.provider_id,
            quality.unwrap_or("-")
        ),
    );

    let first = generate_once(&route.gen_url, api_key, prompt, model, size, quality);
    if let Err(e) = &first {
        crate::ulog::write("draw", &format!("主模型 {model} 失败：{e}"));
    }

    // 两种「换个模型就能好」的失败自动重画一次，客户零操作。**兜到不同的模型**，理由见两个常量的注释。
    //
    // 安全拒绝这条对文生图同样必要，别以为只有改真人照片才会踩：2026-07-28 实测
    // 「烧烤店海鲜烤串宣传图」这么一句人畜无害的话就被 Azure 安全系统拒了（同一句写详细点又能过，
    // qwen 侧完全没问题）——纯误杀。客户看到「安全系统拒绝了这个提示词」只会一头雾水。
    //
    // 🔴 **只在默认（虾盘云）路径上兜底**。两个兜底模型 id 是按虾盘云的渠道路由表挑的
    // （见两个常量的注释：qwen-image 走阿里直连、seedream 走图像中转 A）——
    // 客户自己的端点上多半根本没有这两个 id，兜过去只会把一个能看懂的上游报错换成
    // 「model not found」，还白等一轮 10 分钟。
    let fallback_model = match &first {
        _ if !route.builtin => None,
        Err(e) if is_upstream_outage(e) => Some(OUTAGE_FALLBACK_GEN_MODEL),
        Err(e) if is_safety_rejection(e) => Some(SAFETY_FALLBACK_EDIT_MODEL),
        _ => None,
    }
    .filter(|fb| *fb != model);

    let result = match (first, fallback_model) {
        (Ok(img), _) => Ok(img),
        // 兜底模型不认 quality（那是 gpt-image 系专属），传 None。
        (Err(e), Some(fb)) => {
            crate::ulog::write("draw", &format!("触发兜底：{model} → {fb}"));
            match generate_once(&route.gen_url, api_key, prompt, fb, size, None) {
                Ok(mut img) => {
                    crate::ulog::write("draw", &format!("兜底 {fb} 出图成功"));
                    img.fallback_from = Some(friendly_image_model_name(model));
                    Ok(img)
                }
                // 兜底也没成 → 回**原始**报错。别把兜底模型的报错糊给客户，
                // 那会让他以为是自己选的模型坏了（他选的是 gpt-image-2）。
                Err(e2) => {
                    // 两条报错都留痕：客户只看得到原始那条，排障时另一条往往才是真因。
                    crate::ulog::write("draw", &format!("兜底 {fb} 也失败：{e2}"));
                    Err(e)
                }
            }
        }
        (Err(e), None) => Err(e),
    };

    // 统一翻中文：上游 error body（余额不足 / 安全拒绝 / 尺寸不支持）和 curl 层报错都过这一道，
    // 否则英文原文直接糊给客户看（老 bug：friendlier 只套了 curl 层，漏了 body 层）。
    let img = result.map_err(|e| {
        let cn = friendlier_image_error(e, &route);
        crate::ulog::write("draw", &format!("最终失败：{cn}"));
        cn
    })?;
    crate::ulog::write("draw", "出图成功");
    Ok(ensure_b64(img))
}

/// 图生图 / 图片编辑：调虾盘云 edits 端点（multipart）。
///
/// 参考图由前端传 base64（可带 `data:image/...;base64,` 前缀，后端自动剥）。后端把每张图
/// 解码落临时文件，再用系统 curl 的 `-F` 上传，完成后清理。单图用 `image=@`（最兼容）；
/// 多图用 `image[]=@` 重复（gpt-image 系多图融合）。prompt 走 `-F "prompt=<file"` 从文件读，
/// 彻底规避中文/特殊字符的命令行引号与编码地狱（与 JSON 走 @file 同思路）。
pub fn generate_image_edit(
    api_key: &str,
    prompt: &str,
    model: &str,
    size: &str,
    images_b64: &[String],
) -> Result<ImageResult, String> {
    // 与文生图同一条路由（同一个 provider 的 generations / edits 必须成对，不许一半打我们、
    // 一半打别人 —— 那种混合态排障时最费时间）。
    let route = draw_endpoint();
    let api_key = &route.effective_key(api_key)?;
    if prompt.trim().is_empty() {
        return Err("请先输入修改要求（例如：把背景换成星空、改成水彩风）".into());
    }
    if images_b64.is_empty() {
        return Err("请先拖入或选择至少一张参考图".into());
    }
    let model = &route.effective_model(model);
    let model = model.as_str();

    let pid = std::process::id();
    let dir = std::env::temp_dir();
    // 先解码并验证全部输入，再落临时文件。Azure Image Edit 只收 PNG/JPG；先验完可避免
    // 第 N 张非法时，前 N 张临时文件已经写下却因提前 return 永远清不掉。
    let mut decoded: Vec<(Vec<u8>, &'static str)> = Vec::new();
    for b64 in images_b64 {
        // 容忍 data URL：取最后一个逗号之后的纯 base64
        let raw = b64.rsplit(',').next().unwrap_or(b64);
        let bytes = b64_decode(raw)?;
        if bytes.is_empty() {
            continue;
        }
        let ext = image_edit_ext(&bytes)?;
        decoded.push((bytes, ext));
    }
    if decoded.is_empty() {
        return Err("参考图解析失败（图片数据为空或损坏）".into());
    }

    // 参考图落临时文件（扩展名必须与内容一致，上游按文件类型校验）
    let mut paths: Vec<std::path::PathBuf> = Vec::new();
    for (i, (bytes, ext)) in decoded.iter().enumerate() {
        let p = dir.join(format!("uking-ref-{pid}-{i}.{ext}"));
        std::fs::write(&p, bytes).map_err(|e| format!("写参考图临时文件失败: {e}"))?;
        paths.push(p);
    }
    // prompt 落临时文件，-F 从文件读值
    let prompt_file = dir.join(format!("uking-editprompt-{pid}.txt"));
    std::fs::write(&prompt_file, prompt.as_bytes())
        .map_err(|e| format!("写提示词临时文件失败: {e}"))?;

    // 先按用户选的模型跑一次（edit_once 返回**未翻译**的原始报错，便于识别失败类型）。
    let first = edit_once(&route.edit_url, api_key, &prompt_file, model, size, &paths);

    // 两种「换个模型就能好」的失败，自动换一次再试（客户零操作）。**两条路兜到不同的模型**，
    // 因为要解决的问题根本不同：
    //   ① 安全拒绝  → 换审核更宽松的 Seedream（同为中转商无所谓，问题出在审核不在可用性）；
    //   ② 上游挂了  → 必须换**供应商独立**的阿里直连，绝不能兜到同样走图像中转 A 的 Seedream。
    // 换模型无济于事的失败（余额不足 / 尺寸不合法 / 超时）不在这里兜——见 is_upstream_outage 的注释。
    // 非默认路径一律不兜（理由同文生图那处：这两个 id 是虾盘云渠道表里的，别家没有）。
    let fallback_model = match &first {
        _ if !route.builtin => None,
        Err(e) if is_upstream_outage(e) => Some(OUTAGE_FALLBACK_EDIT_MODEL),
        Err(e) if is_safety_rejection(e) => Some(SAFETY_FALLBACK_EDIT_MODEL),
        _ => None,
    }
    .filter(|fb| *fb != model);

    let result = match (first, fallback_model) {
        (Ok(img), _) => Ok(img),
        (Err(e), Some(fb)) => {
            let fb_size = seedream_edit_size(size);
            match edit_once(&route.edit_url, api_key, &prompt_file, fb, &fb_size, &paths) {
                Ok(mut img) => {
                    img.fallback_from = Some(friendly_image_model_name(model));
                    Ok(img)
                }
                // 兜底也没成 → 回**原始**报错。别把兜底模型的报错糊给客户，
                // 那会让他以为是自己选的模型坏了。
                Err(_) => Err(e),
            }
        }
        (Err(e), None) => Err(e),
    };

    // 清理临时文件（无论成败）
    for p in &paths {
        let _ = std::fs::remove_file(p);
    }
    let _ = std::fs::remove_file(&prompt_file);

    // 统一在最后把原始报错翻成中文（安全拒绝 / 余额不足 / 尺寸不支持等）。
    result.map_err(|e| friendlier_image_error(e, &route))
}

/// 跑一次图生图（build multipart args + curl + parse + ensure_b64）。返回**未翻译**的原始报错，
/// 便于上层 `is_safety_rejection` 判断要不要换模型兜底；翻中文由调用方在最后统一做。
fn edit_once(
    edit_url: &str,
    api_key: &str,
    prompt_file: &std::path::Path,
    model: &str,
    size: &str,
    paths: &[std::path::PathBuf],
) -> Result<ImageResult, String> {
    let auth = format!("Authorization: Bearer {api_key}");
    let field = if paths.len() == 1 { "image" } else { "image[]" };
    let model_arg = format!("model={model}");
    let size_arg = format!("size={size}");
    let prompt_arg = format!("prompt=<{}", prompt_file.display());
    let tmo = IMAGE_GEN_TIMEOUT_S.to_string();
    let img_args: Vec<String> = paths.iter().map(|p| format!("{field}=@{}", p.display())).collect();

    // ⚠️ 不传 response_format：gpt-image-2（默认/推荐模型）的 **edits 端点拒绝 response_format**
    // → HTTP 400 "Unknown parameter: 'response_format'"（2026-06-22 Mac 裸网实测）。gpt-image-2 本就
    // 默认回 b64；seedream/wanx 无论传不传都回 url（忽略它）。所以 edits 一律不传，最稳。
    let mut args: Vec<&str> = vec!["-sS", "-m", &tmo, "-X", "POST", edit_url, "-H", &auth];
    args.extend([
        "-F", &model_arg,
        "-F", &prompt_arg,
        "-F", "n=1",
        "-F", &size_arg,
    ]);
    for ia in &img_args {
        args.push("-F");
        args.push(ia);
    }

    let out = curl(&args)?;
    let v: Value = serde_json::from_str(&out)
        .map_err(|_| format!("图生图响应不是 JSON：{}", snippet(&out, 300)))?;
    Ok(ensure_b64(parse_image_response(&v, model)?))
}

/// 解析作图/图生图响应（generations 与 edits 同结构）：取 data[0] 的 b64_json / url / revised_prompt。
fn parse_image_response(v: &Value, model: &str) -> Result<ImageResult, String> {
    if let Some(err) = v.get("error").filter(|e| !e.is_null()) {
        return Err(api_error(err));
    }
    let first = v
        .get("data")
        .and_then(|d| d.as_array())
        .and_then(|arr| arr.first())
        .ok_or_else(|| format!("作图响应缺少 data：{}", snippet(&v.to_string(), 200)))?;
    let b64 = first.get("b64_json").and_then(|x| x.as_str()).map(String::from);
    let url = first.get("url").and_then(|x| x.as_str()).map(String::from);
    if b64.is_none() && url.is_none() {
        return Err(format!("作图响应既无 b64 也无 url：{}", snippet(&v.to_string(), 200)));
    }
    Ok(ImageResult {
        b64,
        url,
        model: model.to_string(),
        revised_prompt: first.get("revised_prompt").and_then(|x| x.as_str()).map(String::from),
        fallback_from: None,
    })
}

/// 上游若只回 url（不回 b64），把图下载下来转 b64 —— 保证历史能落盘、离线也能看。
/// best-effort：下载失败保留 url（前端在线时仍能直接渲染）。
/// 走 curl `-o` 写文件再读字节：`curl` 助手对 stdout 按 utf8 lossy 解会毁二进制，故必须落盘。
fn ensure_b64(mut r: ImageResult) -> ImageResult {
    if r.b64.is_some() {
        return r;
    }
    if let Some(url) = r.url.clone() {
        let tmp = std::env::temp_dir().join(format!("uking-img-{}.bin", std::process::id()));
        let path = tmp.display().to_string();
        // 作图结果常落第三方 CDN（字节 TOS / 阿里 OSS）。这些域名在部分客户机（带代理 / 证书
        // 吊销服务器不可达）会触发 schannel CRYPT_E_REVOCATION_OFFLINE（curl 35）→下载失败。
        // 结果图是上游已授权产物，跳过吊销检查（--ssl-no-revoke）安全；-L 跟 CDN 跳转。
        // 实测：开发机裸 curl 下 volces.com 必 35，加 --ssl-no-revoke 后 200 拿到 290KB JPG。
        if curl(&["-sS", "-m", "60", "-L", "--ssl-no-revoke", "-o", &path, &url]).is_ok() {
            if let Ok(bytes) = std::fs::read(&tmp) {
                if !bytes.is_empty() {
                    r.b64 = Some(b64_encode(&bytes));
                }
            }
        }
        let _ = std::fs::remove_file(&tmp);
    }
    r
}

/// Azure Image Edit 输入契约：只接受 PNG/JPG。GUI 会先经 canvas 统一成 RGBA PNG；
/// 这里是不可绕过的后端门，拦住小程序/旧调用方直接塞 GIF、BMP、WebP 或伪装扩展名。
fn image_edit_ext(bytes: &[u8]) -> Result<&'static str, String> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Ok("png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Ok("jpg")
    } else {
        Err("参考图格式不兼容：图片编辑只支持 RGB/RGBA 的 PNG/JPG。请在 U-King 里重新选择这张图（会自动转换），或先另存为 PNG/JPG。".into())
    }
}

// ── base64（纯 std，图生图参考图解码 + url 结果编码用；与 draw.rs 同款，叶子工具不跨模块耦合）──
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

fn b64_decode(s: &str) -> Result<Vec<u8>, String> {
    let mut rev = [255u8; 256];
    for (i, &c) in B64_CHARS.iter().enumerate() {
        rev[c as usize] = i as u8;
    }
    // 兼容 URL-safe 变体（- _ → + /）
    rev[b'-' as usize] = 62;
    rev[b'_' as usize] = 63;
    let mut buf = 0u32;
    let mut bits = 0;
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    for c in s.bytes() {
        if c == b'=' || c.is_ascii_whitespace() {
            continue;
        }
        let v = rev[c as usize];
        if v == 255 {
            return Err("参考图 base64 含非法字符".into());
        }
        buf = (buf << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    Ok(out)
}

// ============================================================
// 虾盘云余额
// ============================================================

#[derive(Debug, Clone, Serialize)]
pub struct Balance {
    pub tokens: i64,
    pub cny: f64,
    pub text: String,
}

/// 查虾盘云余额：token余额 = hard_limit_usd × 500000。
/// hard_limit_usd 是服务端**已扣过 total_usage 的实时剩余余额**（2026-06-17 用 sk-44fe… 实测：
/// 每次 chat 后 hard_limit_usd 同步下降），客户端**不能再减 total_usage/100**，否则双重扣减→误报「余额不足」。
pub fn query_balance(api_key: &str) -> Result<Balance, String> {
    let auth = format!("Authorization: Bearer {api_key}");
    // 短超时 + connect-timeout：连不上时快速失败，别让余额卡条把整页拖几十秒
    // （此命令已在 spawn_blocking 跑、不占主线程，但前端仍要等结果，超时越短体验越好）。
    let sub = curl(&[
        "-sS",
        "-m",
        "8",
        "--connect-timeout",
        "5",
        "-H",
        &auth,
        "https://api.u-claw.org.cn/v1/dashboard/billing/subscription",
    ])?;
    let usage = curl(&[
        "-sS",
        "-m",
        "8",
        "--connect-timeout",
        "5",
        "-H",
        &auth,
        "https://api.u-claw.org.cn/v1/dashboard/billing/usage?start_date=2020-01-01&end_date=2030-01-01",
    ])?;

    let sub: Value = serde_json::from_str(&sub).map_err(|_| format!("余额响应异常：{}", snippet(&sub, 200)))?;
    let usage: Value =
        serde_json::from_str(&usage).map_err(|_| format!("用量响应异常：{}", snippet(&usage, 200)))?;

    let hard = sub
        .get("hard_limit_usd")
        .and_then(|x| x.as_f64())
        .ok_or("余额响应缺少 hard_limit_usd（Key 是否有效？）")?;
    let used = usage.get("total_usage").and_then(|x| x.as_f64()).unwrap_or(0.0);

    // 记一条用量快照，供「每日消耗」趋势计算
    crate::usage::record(used, hard);

    // 余额可能为负（已用超过充值额度 = 欠费/超用）。产品上不给用户看负数：
    // 钳到 0，并把文案换成「余额不足，请充值」。tokens 字段也钳 0，避免前端再算出负的。
    // ⚠️ hard 已是实时余额（服务端扣过 usage），**不能再减 used/100**（双重扣减 bug，2026-06-17 修）。
    let raw = (hard * 500_000.0).round() as i64;
    let tokens = raw.max(0);
    let text = if tokens <= 0 {
        "余额不足，请充值".to_string()
    } else if tokens >= 10_000 {
        format!("{:.1} 万 token", tokens as f64 / 10_000.0)
    } else {
        format!("{tokens} token")
    };
    Ok(Balance {
        tokens,
        cny: hard.max(0.0),
        text,
    })
}

// ============================================================
// 用量明细（花在哪了）
// ============================================================

#[derive(Debug, Clone, Serialize)]
pub struct UsageBreakdownItem {
    pub model: String,
    pub cny: f64,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct UsageBreakdown {
    pub days: i64,
    pub items: Vec<UsageBreakdownItem>,
}

/// 查「钱花在哪了」——按模型分组的消耗明细。这是虾盘云自建的端点（不是 OpenAI 兼容 API
/// 的一部分，`/v1/dashboard/billing/breakdown`），服务端一条按 token_id 索引的聚合查询，
/// 客户端只在打开 AI 设置页时按需查一次，不轮询、不加服务器负担。
pub fn query_usage_breakdown(api_key: &str, days: i64) -> Result<UsageBreakdown, String> {
    let auth = format!("Authorization: Bearer {api_key}");
    let resp = curl(&[
        "-sS",
        "-m",
        "8",
        "--connect-timeout",
        "5",
        "-H",
        &auth,
        &format!("https://api.u-claw.org.cn/v1/dashboard/billing/breakdown?days={days}"),
    ])?;
    let v: Value = serde_json::from_str(&resp).map_err(|_| format!("用量明细响应异常：{}", snippet(&resp, 200)))?;
    let items = v
        .get("items")
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|it| {
            Some(UsageBreakdownItem {
                model: it.get("model")?.as_str()?.to_string(),
                cny: it.get("cny")?.as_f64()?,
                count: it.get("count").and_then(|c| c.as_i64()).unwrap_or(0),
            })
        })
        .collect();
    Ok(UsageBreakdown { days, items })
}

// 沙箱互斥锁曾经定义在这儿（`pub(crate) SANDBOX_LOCK`），注释也写明了「凡是要改
// UKING_TEST_HOME 的测试模块一律锁这一把」—— 但后来的模块复制 `with_sandbox` 时
// 连注释一起抄走、各自新起了一把本地锁，等于没锁。现已下沉到 `crate::testsandbox`，
// 那里是全进程唯一的一把。**别在本文件里再起第二把。**

#[cfg(test)]
mod provider_list_ownership_tests {
    use super::*;

    /// 每个用例一个独立沙箱（`UKING_TEST_HOME`），绝不碰开发机真实的 ~/.uking。
    fn with_sandbox(tag: &str, f: impl FnOnce()) {
        crate::testsandbox::with_sandbox(&format!("provider-list-{tag}"), &[".uking"], |_| f())
    }

    fn ids() -> Vec<String> {
        list_providers().into_iter().map(|p| p.id).collect()
    }

    /// 一个只有 OpenAI 端点的中转（`anthropic_base: None`）—— issue #359 客户机上的形状。
    /// `id` 传空 = 让后端自己生成（中文名撞 id 那条用例正是要验这个）。
    fn openai_only(id: &str, name: &str) -> ProviderPreset {
        ProviderPreset {
            id: id.into(),
            name: name.into(),
            summary: String::new(),
            openai_base: "https://relay.example.com/v1".into(),
            anthropic_base: None,
            model: "demo-model".into(),
            small_model: "demo-model".into(),
            codex_model: String::new(),
            codex_wire_api: WIRE_API.into(),
            key_url: String::new(),
            key_hint: String::new(),
            builtin_recharge: false,
            recommended: false,
            builtin: false,
            api_key: "sk-demo".into(),
        }
    }

    /// ★ 核心承诺：移除内置驱动后，**再怎么读列表它都不回来**。
    ///
    /// `list_providers()` 每次都重新读盘，所以连着读两次等价于「重启后再看一次」——
    /// 墓碑要是没落盘（比如只存在内存里），第二次读就会把虾盘云带回来。
    #[test]
    fn removed_builtin_never_comes_back() {
        with_sandbox("tombstone", || {
            assert!(ids().contains(&"xiapan".to_string()), "干净沙箱里本该有虾盘云");
            remove_provider_for(None, "xiapan").unwrap();
            assert!(!ids().contains(&"xiapan".to_string()), "移除后不该还在列表里");
            // 重新读盘 = 下次开机再看一眼
            assert!(!ids().contains(&"xiapan".to_string()), "重读后又回来了 —— 墓碑没落盘");
            assert_eq!(hidden_ids_for(None), vec!["xiapan".to_string()]);
        });
    }

    /// 全删光就是空的（只剩「官方直连」那个出口）—— 不许自动补种任何东西。
    #[test]
    fn deleting_everything_leaves_an_empty_list() {
        with_sandbox("delete-all", || {
            for id in ids() {
                if id != "official" {
                    remove_provider_for(None, &id).unwrap();
                }
            }
            assert_eq!(ids(), vec!["official".to_string()], "删光后只该剩「官方直连（还原）」");
        });
    }

    /// ★ 回归钉子（issue #359 火山方舟豆包 / #322 一个叫 Claude 的中转）：
    /// **纯 OpenAI 兼容的供应商配不了 Claude Code，但不该因此把别的工具一起拖下水。**
    ///
    /// 装机向导会一次传 `claude,codex`（targetsFromInstalled）。老行为是 `apply_claude(..)?`
    /// 直接抛错 → 整次写入失败 → 向导 retryDriver 打转，客户卡在首装最后一米，
    /// 而 Codex 那半边其实完全能配好。
    #[test]
    fn openai_only_provider_does_not_sink_the_other_tools() {
        with_sandbox("cap-mismatch", || {
            save_custom_provider(openai_only("custom-ark", "火山方舟（豆包）")).unwrap();

            assert!(
                !supports_claude_code(&preset("custom-ark").unwrap()),
                "没有 Anthropic 端点就是驱动不了 Claude Code",
            );

            // 多目标：claude 跳过，codex 必须真配上
            let r = apply_provider("custom-ark", "sk-test", None, &["claude".into(), "codex".into()])
                .expect("能力不匹配不该让整次写入失败");
            assert!(
                r.claude.as_deref().unwrap_or("").contains("已跳过"),
                "claude 该被跳过并说明原因，实际：{:?}",
                r.claude,
            );
            assert!(r.codex.is_some(), "codex 本来就能配，不该被连累");

            // **跳过 ≠ 生效**：回显绝不能把它记成 Claude Code 在用的驱动
            let active = driver_status().active;
            assert_ne!(
                active.get("claude").map(String::as_str),
                Some("custom-ark"),
                "没配上却记成在用 —— 回显撒谎比配不上更糟",
            );
            assert_eq!(
                active.get("codex").map(String::as_str),
                Some("custom-ark"),
                "codex 真配上了就该记上",
            );

            // 单目标（用户在 AI 设置里专门点 Claude Code）：必须老老实实报错，
            // 不许静默什么都不做还报成功。
            assert!(
                apply_provider("custom-ark", "sk-test", None, &["claude".into()]).is_err(),
                "只点 Claude Code 时该明确失败，而不是假装成功",
            );
        });
    }

    /// ★ 回归钉子（issue #359）：**中文名供应商不许撞同一个 id**。
    ///
    /// 老的前端自己算 slug（`name.replace(/[^a-z0-9]+/g,"-")`），中文名整串变 "-" → id 恒为
    /// `custom--`；后端 slugify 兜底成 `custom-provider`，同样人人相同。而保存是 upsert 按 id，
    /// 于是**第二个中文名供应商会静默覆盖第一个**，用户看到的是「我加的那个不见了」。
    #[test]
    fn chinese_named_providers_get_distinct_ids() {
        with_sandbox("cjk-id", || {
            // id 传空 = 交给后端生成（前端那份自算 slug 的实现已删）
            let mk = |name: &str| save_custom_provider(openai_only("", name)).unwrap();
            let a = mk("火山方舟（豆包）");
            let b = mk("智谱清言");
            let c = mk("通义千问");

            assert_ne!(a.id, b.id, "两个中文名撞了同一个 id");
            assert_ne!(b.id, c.id, "两个中文名撞了同一个 id");
            for p in [&a, &b, &c] {
                assert!(!p.id.trim_end_matches('-').is_empty(), "id 不该是空壳：{:?}", p.id);
            }
            // 三个都还在 —— 没有谁把谁覆盖掉
            let saved: Vec<String> = read_custom_providers().into_iter().map(|p| p.name).collect();
            for n in ["火山方舟（豆包）", "智谱清言", "通义千问"] {
                assert!(saved.contains(&n.to_string()), "{n} 被覆盖没了：{saved:?}");
            }
        });
    }

    /// 「官方直连（还原）」是还原成官方登录的出口，删不得 —— 删了用户就没退路。
    #[test]
    fn official_restore_exit_cannot_be_removed() {
        with_sandbox("official-guard", || {
            assert!(remove_provider_for(None, "official").is_err(), "官方还原出口不该能删");
            assert!(ids().contains(&"official".to_string()));
        });
    }

    /// 加回来只能靠用户显式调 restore；自定义是真删，加不回来（只能重填）。
    #[test]
    fn restore_is_the_only_way_back() {
        with_sandbox("restore", || {
            remove_provider_for(None, "xiapan").unwrap();
            restore_provider_for(None, "xiapan").unwrap();
            assert!(ids().contains(&"xiapan".to_string()), "用户点了添加就该回来");
            restore_provider_for(None, "xiapan").unwrap(); // 幂等
            assert_eq!(ids().iter().filter(|i| *i == "xiapan").count(), 1, "别加出两份");
            assert!(restore_provider_for(None, "custom-gone").is_err(), "自定义删了就是删了，不该能恢复");
        });
    }

    /// ★ 默认列表只有两项：虾盘云（我们的生意）+ 官方直连（用户的退路）。
    ///
    /// DeepSeek / GLM / Kimi 在「添加供应商」的模板里本来就有，Ollama 有「本地大模型」页 ——
    /// 同一件事在两个地方各摆一遍，对小白是纯干扰。它们的**定义**没删（`all_providers` 还在），
    /// 只是不占默认列表的位置。
    #[test]
    fn default_list_is_only_xiapan_and_official() {
        with_sandbox("default-two", || {
            assert_eq!(ids(), vec!["xiapan".to_string(), "official".to_string()]);
            // 定义仍在 —— 切驱动/回显/托盘按 id 找得到，不是把功能删了
            let all: Vec<String> = all_providers().into_iter().map(|p| p.id).collect();
            for id in SECONDARY_BUILTINS {
                assert!(all.contains(&id.to_string()), "{id} 的定义不该被删掉");
            }
        });
    }

    /// 内置预设**不许**再混进「某家免费模型」这种以周计寿命的东西 —— 它们的正确落点是
    /// 模板画廊（`src/lib/providerTemplates.ts` + skill 清单的 `provider_templates`，热下发）。
    /// 内置这一层只该有：我们的生意（虾盘云）、用户的退路（官方直连）、以及几家一线大厂。
    /// 2026-08-24 加过一个 `zen-free` 当天就退了，这条用例是那次的墓碑：再想加，先回答
    /// 「为什么它非得发版才能改」。
    #[test]
    fn builtin_presets_carry_no_free_trial_channels() {
        let ids: Vec<String> = builtin_providers().into_iter().map(|p| p.id).collect();
        assert!(!ids.contains(&"zen-free".to_string()), "免费尝鲜渠道走模板热下发，不进内置预设");
        assert!(!SECONDARY_BUILTINS.contains(&"zen-free"), "SECONDARY_BUILTINS 里也要一并摘掉");
    }

    /// 默认不摆出来的那几个：`addable_builtins` 里能看到，加了才进列表，**且要落盘**
    /// （只清墓碑不记 `shown` 的话，重读列表就又被过滤掉 = 加了个寂寞）。移除后同样不回来。
    #[test]
    fn secondary_builtin_shows_up_only_after_user_adds_it() {
        with_sandbox("secondary", || {
            assert!(!ids().contains(&"glm".to_string()), "默认不该摆出 GLM");
            let addable: Vec<String> = addable_for(None).into_iter().map(|p| p.id).collect();
            assert!(addable.contains(&"glm".to_string()), "「添加供应商」里得能找到它");

            restore_provider_for(None, "glm").unwrap();
            assert!(ids().contains(&"glm".to_string()), "用户点了添加就该进列表");
            assert!(ids().contains(&"glm".to_string()), "重读就没了 —— shown 没落盘");
            assert!(
                !addable_for(None).iter().any(|p| p.id == "glm"),
                "已经在列表里了，不该还挂在「可添加」里"
            );

            remove_provider_for(None, "glm").unwrap();
            assert!(!ids().contains(&"glm".to_string()), "移除后不该还在");
            assert!(!ids().contains(&"glm".to_string()), "重读又回来了 —— shown 没清干净");
        });
    }

    /// ★ 存量客户保护：已经切到 GLM/Kimi 的机器升级上来，列表里**必须还看得见它在用的那个**。
    /// 否则用户会以为配置丢了，回头再配一遍 —— 比多显示一行糟糕得多。
    #[test]
    fn in_use_secondary_builtin_stays_visible() {
        with_sandbox("in-use", || {
            record_active_driver("claude", "glm"); // 模拟老客户切过 GLM
            assert!(ids().contains(&"glm".to_string()), "在用的驱动不该从列表里消失");
            // 但用户亲手移除仍然说了算 —— 主权归用户，在用也不例外
            remove_provider_for(None, "glm").unwrap();
            assert!(!ids().contains(&"glm".to_string()), "用户删了就得走，哪怕还在用");
        });
    }

    /// 排在第一位的就是首选 —— 顺序归用户，且要落盘（下次开机还是这个顺序）。
    #[test]
    fn user_order_is_persisted() {
        with_sandbox("order", || {
            let mut want = ids();
            want.reverse();
            set_provider_order_for(None, want.clone()).unwrap();
            assert_eq!(ids(), want, "顺序没按用户排的来");
        });
    }

    /// ★「一键配好全部」只配用户勾的那几个 —— **没勾的一个字节都不动**。
    ///
    /// 这条钉的是 0.9.84 那个原则冲突：这个动作以前无差别覆盖全部已装工具，
    /// 客户没机会说「别碰我的 Codex」。`targets` 是用户意图，只做减法。
    #[test]
    fn apply_everywhere_only_touches_picked_tools() {
        with_sandbox("scope", || {
            let settings = claude_settings_path();
            std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
            let mine = r#"{"env":{"ANTHROPIC_AUTH_TOKEN":"sk-my-own"}}"#;
            std::fs::write(&settings, mine).unwrap();

            // 用户只勾了 hermes → 哪怕这台机器真装着 claude，也不许碰它的配置
            let only = vec!["hermes".to_string()];
            let _ = apply_xiapan_everywhere("xiapan", "sk-xp-test", None, Some(&only));
            assert_eq!(
                std::fs::read_to_string(&settings).unwrap(),
                mine,
                "用户没勾 Claude Code，它的配置却被改了"
            );

            // 一个都没勾 = 明确拒绝，而不是静默成功什么也没干（那种「点了没反应」最难查）
            assert!(apply_xiapan_everywhere("xiapan", "sk-xp-test", None, Some(&[])).is_err());
        });
    }

    fn ids_of(tool: &str) -> Vec<String> {
        list_providers_for(Some(tool)).into_iter().map(|p| p.id).collect()
    }

    /// 一个只有 OpenAI 端点的自定义中转站，用来验「per-tool 移除不销毁定义」。
    fn demo_custom() -> String {
        save_custom_provider(ProviderPreset {
            id: String::new(),
            name: "relay-demo".into(),
            summary: String::new(),
            openai_base: "https://relay.example.com/v1".into(),
            anthropic_base: None,
            model: "gpt-4o".into(),
            small_model: "gpt-4o".into(),
            codex_model: String::new(),
            codex_wire_api: WIRE_API.into(),
            key_url: String::new(),
            key_hint: String::new(),
            builtin_recharge: false,
            recommended: false,
            builtin: false,
            api_key: "sk-relay-demo".into(),
        })
        .unwrap()
        .id
    }

    /// ★ 本次改动的核心承诺：**每个 AI 一份列表**。在 Claude Code 那页删掉虾盘云，
    /// Hermes / Codex / ClawX 那三页照样留着 —— 客户原话「Claude Code 的删除，Hermes 的留下来」。
    ///
    /// 这条一红就是回到了「一删全删」：四个 AI 本来各配各的驱动，共用一份列表等于把
    /// 「我不想让 Claude 用它」执行成「我不想再用它」。
    #[test]
    fn removing_from_one_tool_leaves_the_others_alone() {
        with_sandbox("per-tool-remove", || {
            remove_provider_for(Some("claude"), "xiapan").unwrap();
            assert!(!ids_of("claude").contains(&"xiapan".to_string()), "Claude Code 那页该没了");
            for other in ["codex", "clawx", "hermes"] {
                assert!(
                    ids_of(other).contains(&"xiapan".to_string()),
                    "{other} 那页被连坐删掉了 —— 又回到「一删全删」"
                );
            }
            // 重读一次 = 下次开机再看一眼，两边都得还是这个样子
            assert!(!ids_of("claude").contains(&"xiapan".to_string()), "墓碑没落到 claude 名下");
            assert!(ids_of("hermes").contains(&"xiapan".to_string()), "重读后 hermes 也没了");
            // 加回来也只加回那一个 AI
            restore_provider_for(Some("claude"), "xiapan").unwrap();
            assert!(ids_of("claude").contains(&"xiapan".to_string()));
        });
    }

    /// 打错工具名当场报错 —— 静默去改另一份偏好比报错糟得多。
    #[test]
    fn unknown_tool_is_rejected() {
        with_sandbox("per-tool-unknown", || {
            assert!(remove_provider_for(Some("cluade"), "xiapan").is_err(), "拼错的工具名该被拒");
            assert!(ids_of("claude").contains(&"xiapan".to_string()), "被拒了就一个字节都别改");
        });
    }

    /// ★ 存量客户升级：0.9.8x 的那份**全局**偏好还在，四个 AI 都得先按它显示（升级前后一字不变），
    /// 谁先被改谁才分家 —— 不能因为加了 per-tool 就把老用户删掉的东西又摆回来。
    #[test]
    fn old_global_prefs_carry_over_until_a_tool_is_touched() {
        with_sandbox("per-tool-migrate", || {
            // 手写一份 0.9.8x 形态的偏好文件（没有 tools 段）
            std::fs::write(
                prefs_path(),
                r#"{"hidden":["xiapan"],"shown":[],"order":[]}"#,
            )
            .unwrap();
            for tool in LIST_TOOLS {
                assert!(
                    !ids_of(tool).contains(&"xiapan".to_string()),
                    "{tool}：老用户删掉的东西升级后又冒出来了"
                );
            }
            // 只在 hermes 里加回来 → 其余三个仍按老偏好走
            restore_provider_for(Some("hermes"), "xiapan").unwrap();
            assert!(ids_of("hermes").contains(&"xiapan".to_string()));
            for other in ["claude", "codex", "clawx"] {
                assert!(!ids_of(other).contains(&"xiapan".to_string()), "{other} 被顺手改了");
            }
        });
    }

    /// per-tool 移除自定义供应商**只是从这个 AI 的列表拿走**，定义和 Key 都还在（别的 AI 还在用它）；
    /// 要连定义带 Key 一起删是另一条路（不带 tool = 界面上的「彻底删除」）。
    #[test]
    fn per_tool_removal_keeps_the_custom_definition() {
        with_sandbox("per-tool-custom", || {
            let id = demo_custom();
            remove_provider_for(Some("claude"), &id).unwrap();
            assert!(!ids_of("claude").contains(&id), "Claude Code 那页该没了");
            assert!(ids_of("codex").contains(&id), "别的 AI 不该被连坐");
            assert!(
                all_providers().iter().any(|p| p.id == id),
                "只是移出一个列表，定义和 Key 不该被销毁"
            );
            // 定义还在 → 能加回来（彻底删过的才加不回来）
            restore_provider_for(Some("claude"), &id).unwrap();
            assert!(ids_of("claude").contains(&id));

            // 不带 tool = 彻底删：所有 AI 的列表都没了，定义也没了
            remove_provider_for(None, &id).unwrap();
            for tool in LIST_TOOLS {
                assert!(!ids_of(tool).contains(&id), "{tool}：彻底删除后还留着一行");
            }
            assert!(!all_providers().iter().any(|p| p.id == id), "定义该跟着删掉");
            assert!(restore_provider_for(Some("claude"), &id).is_err(), "彻底删过的不该能恢复");
        });
    }

    /// 顺序也是 per-tool 的：在 Codex 里把谁排前面，不该动 Claude Code 的排法。
    #[test]
    fn order_is_per_tool_too() {
        with_sandbox("per-tool-order", || {
            let base = ids_of("codex");
            let mut want = base.clone();
            want.reverse();
            set_provider_order_for(Some("codex"), want.clone()).unwrap();
            assert_eq!(ids_of("codex"), want, "Codex 的顺序没按用户排的来");
            assert_eq!(ids_of("claude"), base, "Claude Code 的顺序被顺手改了");
        });
    }

    /// 移除某个供应商**不许顺手改客户机器上的 AI 工具配置** ——
    /// 「移除虾盘云」和「把 Claude Code 还原成官方」是两件事，连坐就是又一次替用户做主。
    #[test]
    fn removing_a_provider_touches_no_tool_config() {
        with_sandbox("no-side-effect", || {
            let settings = claude_settings_path();
            std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
            let mine = r#"{"env":{"ANTHROPIC_BASE_URL":"https://api.u-claw.org.cn"}}"#;
            std::fs::write(&settings, mine).unwrap();
            remove_provider_for(None, "xiapan").unwrap();
            assert_eq!(
                std::fs::read_to_string(&settings).unwrap(),
                mine,
                "移除列表项时动了客户已配好的 Claude Code 配置"
            );
        });
    }
}

/// 「恢复官方直连」这条路只有一个不可退让的承诺：**别把客户的官方登录搞没了**。
/// 这几条用例就是拿真实文件形态钉住它 —— 历史上这里删过客户的 ChatGPT 登录。
#[cfg(test)]
mod reset_codex_tests {
    use super::*;

    /// 每个用例一个独立沙箱（`UKING_TEST_HOME`），绝不碰开发机真实的 ~/.codex。
    /// 闭包拿到的是沙箱里的 `.codex` 目录。
    fn with_sandbox(tag: &str, f: impl FnOnce(&std::path::Path)) {
        crate::testsandbox::with_sandbox(&format!("reset-codex-{tag}"), &[".codex"], |root| {
            f(&root.join(".codex"))
        })
    }

    /// ★ 回归钉子：Codex 桌面版走 ChatGPT 官方登录时，auth.json 里**天生就有**
    /// `OPENAI_API_KEY` 这个字符串（值是 null）。老代码拿这个关键字判「是我们写的」，
    /// 于是把客户的官方登录整个删掉。还原之后 tokens 必须一个字节都不少。
    #[test]
    fn official_chatgpt_login_survives_reset() {
        with_sandbox("official", |codex| {
            let auth = codex.join("auth.json");
            let original = r#"{"OPENAI_API_KEY":null,"tokens":{"access_token":"at-real","refresh_token":"rt-real"},"last_refresh":"2026-08-01"}"#;
            std::fs::write(&auth, original).unwrap();

            reset_codex().unwrap();

            assert!(auth.exists(), "官方登录的 auth.json 被删了 —— 客户得重新扫码登录");
            let after: Value = serde_json::from_str(&std::fs::read_to_string(&auth).unwrap()).unwrap();
            assert_eq!(after["tokens"]["access_token"], "at-real", "登录 token 被动过");
            assert_eq!(after["tokens"]["refresh_token"], "rt-real", "刷新 token 被动过");
        });
    }

    /// chat 链路当初是 merge 进一个 key 的，还原就把这一个 key merge 出来：
    /// 文件还在、其它字段（含 tokens）不动。进出对称。
    #[test]
    fn our_api_key_is_removed_but_file_and_tokens_stay() {
        with_sandbox("mergeout", |codex| {
            let auth = codex.join("auth.json");
            std::fs::write(
                &auth,
                r#"{"OPENAI_API_KEY":"sk-uking-written","tokens":{"access_token":"at-real"}}"#,
            )
            .unwrap();

            reset_codex().unwrap();

            assert!(auth.exists(), "不该删文件，只该摘掉那一个键");
            let after: Value = serde_json::from_str(&std::fs::read_to_string(&auth).unwrap()).unwrap();
            assert!(after.get("OPENAI_API_KEY").is_none(), "我们写的 key 没摘干净");
            assert_eq!(after["tokens"]["access_token"], "at-real", "顺手把别人的东西也删了");
        });
    }

    /// 客户自己手写的 config.toml（没有我们的标记）一个字都不许动 ——
    /// 老代码只要看到 `OPENAI_API_KEY` 就删，而 `env_key = "OPENAI_API_KEY"` 是官方文档里的写法。
    #[test]
    fn user_own_config_toml_is_never_deleted() {
        with_sandbox("usercfg", |codex| {
            let cfg = codex.join("config.toml");
            let own = "model = \"gpt-5\"\n[model_providers.mine]\nenv_key = \"OPENAI_API_KEY\"\n";
            std::fs::write(&cfg, own).unwrap();

            reset_codex().unwrap();

            assert!(cfg.exists(), "客户自己的 config.toml 被删了");
            assert_eq!(std::fs::read_to_string(&cfg).unwrap(), own, "客户自己的配置被改了");
        });
    }

    /// 我们自己写的 config.toml（带标记）没备份时该删干净 —— 别矫枉过正把残留留给客户。
    #[test]
    fn our_own_config_toml_is_cleaned_up() {
        with_sandbox("ourcfg", |codex| {
            let cfg = codex.join("config.toml");
            std::fs::write(&cfg, "# managed by U-King\nmodel = \"gpt-5.3-codex\"\n").unwrap();
            reset_codex().unwrap();
            assert!(!cfg.exists(), "我们自己写的配置该清掉");
        });
    }

    /// 「一键配好全部 AI」不许覆盖客户已有的官方登录 / 自己的配置；
    /// 但我们接管过的（带标记）要能幂等重配，否则客户第二次点会莫名其妙被跳过。
    #[test]
    fn auto_config_yields_to_existing_setup() {
        with_sandbox("guard-official", |codex| {
            std::fs::write(
                codex.join("auth.json"),
                r#"{"OPENAI_API_KEY":null,"tokens":{"access_token":"at"}}"#,
            )
            .unwrap();
            assert!(codex_auto_config_blocked().is_some(), "官方登录在，自动流程不该动 Codex");
        });
        with_sandbox("guard-usercfg", |codex| {
            std::fs::write(codex.join("config.toml"), "model = \"gpt-5\"\n").unwrap();
            assert!(codex_auto_config_blocked().is_some(), "客户自己的配置在，自动流程不该覆盖");
        });
        with_sandbox("guard-ours", |codex| {
            std::fs::write(codex.join("config.toml"), "# managed by U-King\nmodel = \"x\"\n").unwrap();
            std::fs::write(codex.join("auth.json"), r#"{"tokens":{"access_token":"at"}}"#).unwrap();
            assert!(codex_auto_config_blocked().is_none(), "我们接管过的应当允许幂等重配");
        });
        with_sandbox("guard-empty", |_| {
            assert!(codex_auto_config_blocked().is_none(), "干净机器上不该拦");
        });
    }

    /// 有备份就以备份为准（备份是「接管前那一份」的真相源）。
    #[test]
    fn backup_wins_when_present() {
        with_sandbox("bak", |codex| {
            let auth = codex.join("auth.json");
            std::fs::write(&auth, r#"{"OPENAI_API_KEY":"sk-ours"}"#).unwrap();
            std::fs::write(codex.join("auth.json.uking-bak"), r#"{"tokens":{"access_token":"at-old"}}"#).unwrap();

            reset_codex().unwrap();

            let after: Value = serde_json::from_str(&std::fs::read_to_string(&auth).unwrap()).unwrap();
            assert_eq!(after["tokens"]["access_token"], "at-old", "没回滚到备份那一份");
        });
    }
}

/// ★ **Hermes 落点** —— pc-*** 那个 404 的地基。
///
/// 这组用例钉死一件事：**Hermes 的家目录由 Hermes 的源码说了算，不由我们的观察说了算。**
/// 它的 `hermes_constants.py::get_hermes_home()` 只有两档 —— `HERMES_HOME` → `~/.hermes`。
/// 我们曾经在中间塞了个 `%LOCALAPPDATA%\hermes`（那其实是**安装目录**），于是把虾盘云端点
/// 写进了一个永远不会被读的地方，客户机上模型名是我们的、端点是别人的 → 拿虾盘云的模型名
/// 打 DeepSeek 官方 → 404，两台客户机同一报错。
///
/// 🔴 **为什么非得有这组用例**：这个 bug 让 `cargo check`、`action conformance`、
/// `--selfcheck` **全绿** —— 我们写文件成功了、形状也对，只是写在了没人读的地方。
/// 唯一能把它照出来的判据就是「落点等于 Hermes 自己算出来的那个」。
#[cfg(test)]
mod hermes_home_tests {
    use super::*;

    /// 造一个沙箱，同时准备好「旧落点」和「真 home」两个目录。
    ///
    /// 宿主 shell 里的 `HERMES_HOME` 残留曾让我们把结论**测反过一次**
    /// （Y:\compare-upstream）—— 清理这个变量的活现在由 `testsandbox` 统一做，
    /// 它进出沙箱都会存档还原，见 `MANAGED_VARS`。
    fn with_sandbox(tag: &str, f: impl FnOnce(&std::path::Path)) {
        crate::testsandbox::with_sandbox(
            &format!("hermes-home-{tag}"),
            &[".hermes", "LocalAppData/hermes"],
            f,
        )
    }

    /// U-King 写配置的落点，必须**不是**旧的那个安装目录。
    /// 这条防的是「哪天有人觉得 `%LOCALAPPDATA%\hermes` 看着更像配置目录」而把它加回来。
    #[test]
    fn write_target_is_never_the_legacy_install_dir() {
        with_sandbox("target", |root| {
            let live = hermes_dir();
            assert_eq!(live, root.join(".hermes"), "落点必须是 Hermes 自己会读的 home");
            let legacy = crate::installer::hermes_legacy_dir();
            assert_ne!(
                Some(live),
                legacy,
                "旧落点（安装目录）绝不能再当写入目标 —— 那正是 pc-*** 404 的根因"
            );
        });
    }

    /// 客户机自愈：旧落点里我们写的那份好配置，要能搬到 Hermes 真会读的 home。
    /// 光把落点改对是不够的 —— 客户升级后若不再点一次「一键配好」，真 home 里的坏配置还在。
    #[test]
    fn migrates_our_config_out_of_the_legacy_dir() {
        with_sandbox("migrate", |root| {
            let legacy = root.join("LocalAppData").join("hermes");
            // 旧落点 = U-King 写的（带我们的标记 + 虾盘云端点）
            std::fs::write(
                legacy.join("config.yaml"),
                "model:\n  active_profile: U-King 虾盘云\n  provider: custom\n  \
                 base_url: https://api.u-claw.org.cn/v1\n  default: deepseek-v4-flash\n",
            )
            .unwrap();
            std::fs::write(legacy.join(".env"), "OPENAI_API_KEY=sk-xp-test\n").unwrap();
            // 真 home = 客户机上那份坏的：**模型名是我们的、端点是 DeepSeek 官方的**
            let live = root.join(".hermes");
            std::fs::write(
                live.join("config.yaml"),
                "model:\n  provider: custom\n  base_url: https://api.deepseek.com/v1\n  \
                 default: deepseek-v4-flash\n",
            )
            .unwrap();
            std::fs::write(live.join(".env"), "OPENAI_BASE_URL=https://api.deepseek.com/v1\n").unwrap();

            assert!(migrate_hermes_config_from_legacy().is_some(), "该搬的时候要真搬");

            let cfg = std::fs::read_to_string(live.join("config.yaml")).unwrap();
            let env = std::fs::read_to_string(live.join(".env")).unwrap();
            assert_eq!(
                read_hermes_model_key(&cfg, "base_url").as_deref(),
                Some("https://api.u-claw.org.cn/v1"),
            );
            // 🔴 .env 才是 Hermes 真正认凭据/端点的地方（provider=custom 走 env/config 兜底）。
            // 只改 config.yaml 而漏了这里，客户机上照样打 DeepSeek 官方 → 还是 404。
            assert_eq!(
                read_env_var(&env, "OPENAI_BASE_URL").as_deref(),
                Some("https://api.u-claw.org.cn/v1"),
            );
            assert_eq!(read_env_var(&env, "OPENAI_API_KEY").as_deref(), Some("sk-xp-test"));

            // 幂等：每次启动都会跑，第二遍必须是 no-op（否则天天写盘 + 天天刷备份）
            assert!(migrate_hermes_config_from_legacy().is_none(), "已经对了就别再写");
        });
    }

    /// **只搬自己的东西**：旧落点里若是用户/别家工具写的配置（没有 U-King 标记），一个字节都不碰。
    #[test]
    fn never_touches_configs_we_did_not_write() {
        with_sandbox("foreign", |root| {
            let legacy = root.join("LocalAppData").join("hermes");
            std::fs::write(
                legacy.join("config.yaml"),
                "model:\n  active_profile: DeepSeek Direct\n  provider: deepseek\n  \
                 base_url: https://api.deepseek.com/v1\n  default: deepseek-chat\n",
            )
            .unwrap();
            let live = root.join(".hermes");
            std::fs::write(live.join("config.yaml"), "model:\n  default: kimi-k2.5\n").unwrap();

            assert!(migrate_hermes_config_from_legacy().is_none());
            let cfg = std::fs::read_to_string(live.join("config.yaml")).unwrap();
            assert_eq!(read_hermes_model_key(&cfg, "default").as_deref(), Some("kimi-k2.5"));
        });
    }

    /// pc-*** 客户机实锤的 404 根因：老配置 `api_mode: anthropic_messages` + OpenAI 端点
    /// （`https://api.deepseek.com/v1` 只有 /chat/completions，Hermes 拼 /v1/messages → 404）。
    /// apply_hermes 必须按 api_mode 选端点：anthropic → anthropic_base（DeepSeek 官方
    /// `https://api.deepseek.com/anthropic` / 虾盘云 `api.u-claw.org` 均提供），
    /// 且 `.env` 的 OPENAI_BASE_URL（Hermes 真正认端点的位置）必须同步。
    #[test]
    fn apply_hermes_uses_anthropic_endpoint_when_api_mode_is_anthropic() {
        with_sandbox("hermes-anthropic", |root| {
            let live = root.join(".hermes");
            std::fs::write(
                live.join("config.yaml"),
                "model:\n  provider: custom\n  base_url: https://api.deepseek.com/v1\n  \
                 api_mode: anthropic_messages\n  default: deepseek-v4-flash\n",
            )
            .unwrap();
            std::fs::write(live.join(".env"), "OPENAI_BASE_URL=https://api.deepseek.com/v1\n").unwrap();

            let p = builtin_providers().into_iter().find(|x| x.id == "deepseek").unwrap();
            apply_hermes(&p, "sk-test-123", "deepseek-v4-flash").unwrap();

            let cfg = std::fs::read_to_string(live.join("config.yaml")).unwrap();
            assert_eq!(
                read_hermes_model_key(&cfg, "base_url").as_deref(),
                Some("https://api.deepseek.com/anthropic"),
                "anthropic 模式必须配 Anthropic 兼容端点，否则 /v1/messages 404"
            );
            let env = std::fs::read_to_string(live.join(".env")).unwrap();
            assert_eq!(
                read_env_var(&env, "OPENAI_BASE_URL").as_deref(),
                Some("https://api.deepseek.com/anthropic"),
                ".env 是 Hermes 真正认端点的位置，必须同步"
            );
        });
    }

    /// openai_chat 模式（默认）→ 维持 OpenAI 兼容端点，行为与老版本一致（无回归）。
    #[test]
    fn apply_hermes_keeps_openai_endpoint_for_openai_mode() {
        with_sandbox("hermes-openai", |root| {
            let live = root.join(".hermes");
            std::fs::write(
                live.join("config.yaml"),
                "model:\n  provider: custom\n  base_url: https://api.u-claw.org.cn/v1\n  \
                 api_mode: openai_chat\n  default: deepseek-v4-flash\n",
            )
            .unwrap();

            let p = builtin_providers().into_iter().find(|x| x.id == "deepseek").unwrap();
            apply_hermes(&p, "sk-test-123", "deepseek-v4-flash").unwrap();

            let cfg = std::fs::read_to_string(live.join("config.yaml")).unwrap();
            assert_eq!(
                read_hermes_model_key(&cfg, "base_url").as_deref(),
                Some("https://api.deepseek.com/v1"),
                "openai_chat 模式维持 OpenAI 端点（现状语义，无回归）"
            );
        });
    }

    /// 🔴 客户实锤（2026-08-19）：hermes 打 `POST {base}/v1/responses`，虾盘云中转回
    /// `500 convert_request_failed / not implemented`，重试 3 次耗尽退出。
    ///
    /// 根因不是「U-King 写错了 provider」—— `apply_hermes` 一直硬编码 `provider: custom`，
    /// 全仓 grep 不到任何 responses 系的 provider 名。真因是 **U-King 从来不写 `api_mode`**：
    /// Hermes 在这个键缺失/不认识时会**按主机名猜 transport**，猜中 `codex_responses` 就打
    /// `/v1/responses`（上游自己记着这坑：`hermes_cli/config.py::_API_MODE_ALIASES` 注释、
    /// #66543、线上对 api.actual.inc 实测）。
    ///
    /// 这条钉死两件事：① 配置里一个坏值（codex_responses）必须被**纠回来**，不是被尊重；
    /// ② 全新安装（config 里压根没这个键）也必须写出 api_mode，不能留给猜。
    #[test]
    fn apply_hermes_pins_chat_completions_and_overrides_bad_api_mode() {
        with_sandbox("hermes-apimode-bad", |root| {
            let live = root.join(".hermes");
            // ① 已被写坏成 responses 系
            std::fs::write(
                live.join("config.yaml"),
                "model:\n  provider: custom\n  base_url: https://api.u-claw.org.cn/v1\n  \
                 api_mode: codex_responses\n  default: deepseek-v4-flash\n",
            )
            .unwrap();
            let p = builtin_providers().into_iter().find(|x| x.id == "deepseek").unwrap();
            apply_hermes(&p, "sk-test-123", "deepseek-v4-flash").unwrap();

            let cfg = std::fs::read_to_string(live.join("config.yaml")).unwrap();
            assert_eq!(
                read_hermes_model_key(&cfg, "api_mode").as_deref(),
                Some("chat_completions"),
                "坏的 api_mode 必须被纠回 chat_completions —— 留着它 hermes 就去打 /v1/responses，\
                 中转站 500。apply 的语义是「把这台机器配成能用」，不是「尊重一个会 500 的旧值」"
            );
            assert_eq!(
                read_hermes_model_key(&cfg, "provider").as_deref(),
                Some("custom"),
                "provider 仍然是 custom（这一条本来就对，别在修 api_mode 时改坏它）"
            );
        });
    }

    /// 上一条的另一半，**故意单独一个用例**：合在一起时前半先失败、后半根本跑不到，
    /// 等于这一路从没被变异验证覆盖过（2026-08-19 实测就是这样）。
    ///
    /// 全新安装：config.yaml 压根不存在 —— 这是客户机最常见的路径，也是暴露面最大的一档，
    /// 因为写出来的块里连 `api_mode` 这个键都没有，Hermes 只能按主机名猜 transport。
    #[test]
    fn apply_hermes_writes_api_mode_on_fresh_install() {
        with_sandbox("hermes-apimode-fresh", |root| {
            let live = root.join(".hermes");
            let _ = std::fs::remove_file(live.join("config.yaml"));
            let p = builtin_providers().into_iter().find(|x| x.id == "deepseek").unwrap();
            apply_hermes(&p, "sk-test-123", "deepseek-v4-flash").unwrap();

            let cfg = std::fs::read_to_string(live.join("config.yaml")).unwrap();
            assert_eq!(
                read_hermes_model_key(&cfg, "api_mode").as_deref(),
                Some("chat_completions"),
                "全新安装也必须写出 api_mode —— 缺这个键正是客户那台机器打 /v1/responses 的暴露面"
            );
        });
    }

    /// anthropic 那一档不能被上面那条误伤：端点走 Anthropic 时 api_mode 要跟着是
    /// `anthropic_messages`，一起写、一起对。两者错配就是 pc-*** 那个 404。
    #[test]
    fn apply_hermes_pins_anthropic_api_mode_together_with_endpoint() {
        with_sandbox("hermes-apimode-anthropic", |root| {
            let live = root.join(".hermes");
            std::fs::write(
                live.join("config.yaml"),
                "model:\n  provider: custom\n  base_url: https://api.deepseek.com/v1\n  \
                 api_mode: anthropic_messages\n  default: deepseek-v4-flash\n",
            )
            .unwrap();
            let p = builtin_providers().into_iter().find(|x| x.id == "deepseek").unwrap();
            apply_hermes(&p, "sk-test-123", "deepseek-v4-flash").unwrap();

            let cfg = std::fs::read_to_string(live.join("config.yaml")).unwrap();
            assert_eq!(
                read_hermes_model_key(&cfg, "api_mode").as_deref(),
                Some("anthropic_messages"),
                "anthropic 端点必须配 anthropic_messages —— 错配就是 /v1/messages 404"
            );
            assert_eq!(
                read_hermes_model_key(&cfg, "base_url").as_deref(),
                Some("https://api.deepseek.com/anthropic"),
                "端点和 api_mode 是一对，必须同时正确"
            );
        });
    }

    /// PowerShell `Set-Content -Encoding UTF8` 写的 config 带 BOM 头 —— 客户机上很常见
    /// （Hermes 自己写 no-BOM，但用户手改/工具写过就可能带）。BOM 剥不掉的话
    /// `set_yaml_model_block` 会追加第二个 model 块（YAML 重复顶层键）、`api_mode`
    /// 也读不到（端点错配 404 的另一条路）。这条钉死 BOM 场景。
    #[test]
    fn apply_hermes_strips_bom_like_powershell_writes() {
        with_sandbox("hermes-bom", |root| {
            let live = root.join(".hermes");
            std::fs::write(
                live.join("config.yaml"),
                "\u{feff}model:\n  provider: custom\n  base_url: https://api.deepseek.com/v1\n  \
                 api_mode: anthropic_messages\n  default: deepseek-v4-flash\n",
            )
            .unwrap();

            let p = builtin_providers().into_iter().find(|x| x.id == "deepseek").unwrap();
            apply_hermes(&p, "sk-test-123", "deepseek-v4-flash").unwrap();

            let cfg = std::fs::read_to_string(live.join("config.yaml")).unwrap();
            assert_eq!(
                cfg.matches("model:").count(),
                1,
                "BOM 必须被剥掉，不能追加第二个 model 块（YAML 重复顶层键）"
            );
            assert_eq!(
                read_hermes_model_key(&cfg, "base_url").as_deref(),
                Some("https://api.deepseek.com/anthropic"),
                "BOM 文件里 anthropic 模式同样要配 Anthropic 兼容端点"
            );
        });
    }
}

#[cfg(test)]
mod env_tests {
    use super::{extract_mcp_servers, image_edit_ext, set_env_var};

    #[test]
    fn image_edit_never_passes_gif_to_azure() {
        // Azure Image Edit 只接受 PNG/JPG。旧实现把 GIF 原样落成 .gif 后上传，
        // 上游稳定返回 "Invalid file or mode for image 1"。
        let gif = b"GIF89a\x01\x00\x01\x00";
        let err = image_edit_ext(gif).unwrap_err();
        assert!(err.contains("只支持"));
    }

    #[test]
    fn image_edit_accepts_real_png_and_jpeg_magic() {
        assert_eq!(image_edit_ext(b"\x89PNG\r\n\x1a\nrest"), Ok("png"));
        assert_eq!(image_edit_ext(b"\xff\xd8\xff\xe0rest"), Ok("jpg"));
    }

    #[test]
    fn replaces_active_line_in_place() {
        // 已有活动键 → 原地替换值，保留位置与其它行
        let src = "A=1\nOPENAI_API_KEY=sk-old\nOPENAI_BASE_URL=https://api.deepseek.com/v1\nB=2\n";
        let out = set_env_var(src, "OPENAI_API_KEY", "sk-new");
        assert!(out.contains("OPENAI_API_KEY=sk-new"));
        assert!(!out.contains("sk-old"));
        // 其它行原样保留
        assert!(out.contains("A=1"));
        assert!(out.contains("B=2"));
        assert!(out.contains("OPENAI_BASE_URL=https://api.deepseek.com/v1"));
        // 只出现一次（没重复追加）
        assert_eq!(out.matches("OPENAI_API_KEY=").count(), 1);
    }

    #[test]
    fn enables_commented_line() {
        // 只有注释键 → 在注释后插入启用行，注释保留作参考
        let src = "# OPENROUTER_API_KEY=\nFOO=bar\n";
        let out = set_env_var(src, "OPENROUTER_API_KEY", "sk-x");
        assert!(out.contains("# OPENROUTER_API_KEY="));
        assert!(out.contains("OPENROUTER_API_KEY=sk-x"));
        // 启用行紧跟注释
        let lines: Vec<&str> = out.lines().collect();
        let ci = lines.iter().position(|l| l.trim_start().starts_with("# OPENROUTER_API_KEY=")).unwrap();
        assert_eq!(lines[ci + 1], "OPENROUTER_API_KEY=sk-x");
    }

    #[test]
    fn appends_when_absent() {
        let src = "FOO=bar\n";
        let out = set_env_var(src, "OPENAI_BASE_URL", "https://api.u-claw.org.cn/v1");
        assert!(out.contains("FOO=bar"));
        assert!(out.trim_end().ends_with("OPENAI_BASE_URL=https://api.u-claw.org.cn/v1"));
    }

    /// PowerShell `Set-Content -Encoding UTF8` 写的 .env 带 BOM 头 —— 首行键必须能命中替换，
    /// 而不是重复追加一行（重复键靠 dotenv「后者生效」兜底是脆的，且文件脏）。
    #[test]
    fn bom_line_is_replaced_in_place() {
        let src = "\u{feff}OPENAI_BASE_URL=https://api.deepseek.com/v1\n";
        let out = set_env_var(src, "OPENAI_BASE_URL", "https://api.deepseek.com/anthropic");
        assert_eq!(out.matches("OPENAI_BASE_URL=").count(), 1, "BOM 行要被替换而不是追加第二行");
        assert!(out.contains("OPENAI_BASE_URL=https://api.deepseek.com/anthropic"));
        assert!(!out.contains("api.deepseek.com/v1"));
    }

    #[test]
    fn empty_file_gets_key() {
        let out = set_env_var("", "OPENAI_API_KEY", "sk-z");
        assert_eq!(out, "OPENAI_API_KEY=sk-z\n");
    }

    /// 回归钉子（pc-*** / Issue #222 #223 #318 #319 #323）：
    /// **写完必须回读校验**，「写调用返回 Ok」不等于「盘上那份是我们写的」。
    ///
    /// 那个 bug 之所以能潜伏一周，就是因为「写完就当成功」：日志一路印着「已装好并通过自检」，
    /// 而那句自检说的是**写配置之前**的事实。`atomic_write` 是 providers.rs 全部配置写入的
    /// 唯一收口，这条用例把「回读对不上要报错」钉住。
    #[test]
    fn atomic_write_verifies_what_landed_on_disk() {
        let dir = std::env::temp_dir().join(format!("uking-aw-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("cfg.json");

        // 正常路径：写得进去、读得回来、内容一致。
        let body = br#"{"env":{"ANTHROPIC_BASE_URL":"https://api.example.com"}}"#;
        assert!(super::atomic_write(&p, body).is_ok(), "正常写入不该失败");
        assert_eq!(std::fs::read(&p).unwrap(), body.to_vec());

        // 故障路径：写完之后被别人（杀软/同步盘/另一个程序刷内存副本）改掉 —— 必须报错，
        // 不能静默当成功。这里把目标做成目录来制造「写得成、读不回原样」的确定性失败。
        let _ = std::fs::remove_file(&p);
        let blocked = dir.join("blocked");
        let _ = std::fs::create_dir_all(&blocked);
        let r = super::atomic_write(&blocked, body);
        assert!(r.is_err(), "目标不是我们写的那份内容时必须报错，而不是静默成功");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 切驱动**整文件覆盖** config.toml，而 `codex mcp add` 写的正是这个文件的
    /// `[mcp_servers.*]`（已用临时 CODEX_HOME 实测）。不捞出来带回去，客户每切一次驱动
    /// 就把自己挂的连接器全抹一次，且毫无提示 —— 跟「不许抢客户模型/登录态」同一条红线。
    #[test]
    fn extract_mcp_servers_keeps_user_connectors_and_nothing_else() {
        let src = r#"# managed by U-King
model = "deepseek-v4-flash-codex"
model_provider = "xiapan"

[model_providers.xiapan]
name = "虾盘云"
base_url = "https://api.u-claw.org.cn/v1"

[mcp_servers.memory]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-memory"]
# 用户自己写的注释也要留住

[mcp_servers.playwright]
command = "npx"
args = ["-y", "@playwright/mcp@latest"]

[tui]
theme = "dark"
"#;
        let got = extract_mcp_servers(src);
        assert!(got.contains("[mcp_servers.memory]"), "第一个连接器丢了: {got}");
        assert!(got.contains("[mcp_servers.playwright]"), "第二个连接器丢了: {got}");
        assert!(got.contains("@playwright/mcp@latest"), "段内的键值丢了: {got}");
        assert!(got.contains("# 用户自己写的注释也要留住"), "原样保留 = 连注释一起: {got}");
        // 只捞 mcp 段，别把别人的段也顺走（顺走 = 下面重写时出现重复段，Codex 直接解析失败）
        assert!(!got.contains("model_provider"), "把驱动键也捞进来了: {got}");
        assert!(!got.contains("[model_providers.xiapan]"), "把 provider 段也捞进来了: {got}");
        assert!(!got.contains("[tui]"), "把 mcp 之后的别的段也捞进来了: {got}");
        assert!(!got.contains("theme"), "mcp 段的结束判据没生效: {got}");

        // 没挂过连接器 → 空字符串（调用方据此决定要不要追加那段注释）
        assert_eq!(extract_mcp_servers("model = \"x\"
"), "");
    }
}

/// 长会省钱双键（2026-08-25）的注入契约测试。
///
/// GPT-5.6-sol v3 评审定下的规矩，逐条钉死：
/// B 条 → 只有虾盘云 DeepSeek 族拿默认 200K，官方 Claude 模型一个键都不给；
/// C 条 → 用户已自己配过的值原样保留（让路不覆盖）；
/// #375 红线延伸 → 客户用自己的 Key 时，委派 env 一个字都不注入。
/// 还原（reset_claude）按**归属追踪**办事：只收走带标记（我们注入）的键，
/// 无标记的键——哪怕值恰好等于我们的特征值 200000——一律不碰（sol 复审 P1）。
#[cfg(test)]
mod auto_compact_window_tests {
    use super::*;

    fn with_sandbox(tag: &str, f: impl FnOnce()) {
        crate::testsandbox::with_sandbox(&format!("autocompact-{tag}"), &[".claude"], |_| f())
    }

    fn read_env() -> serde_json::Map<String, Value> {
        let s = std::fs::read_to_string(claude_settings_path()).expect("settings.json 应该存在");
        let root: Value = serde_json::from_str(&s).expect("settings.json 应是合法 JSON");
        root.get("env").unwrap().as_object().cloned().expect("env 应是对象")
    }

    /// 非 DeepSeek 的自定义中转（官方 Anthropic 端点形状）。
    fn official_like(model: &str) -> ProviderPreset {
        ProviderPreset {
            id: "official-like".into(),
            name: "某官方直连".into(),
            summary: String::new(),
            openai_base: "https://api.anthropic.com/v1".into(),
            anthropic_base: Some("https://api.anthropic.com".into()),
            model: model.into(),
            small_model: model.into(),
            codex_model: String::new(),
            codex_wire_api: WIRE_API.into(),
            key_url: String::new(),
            key_hint: String::new(),
            builtin_recharge: false,
            recommended: false,
            builtin: false,
            api_key: "sk-demo".into(),
        }
    }

    /// 🔴 opus P1 + sol 复审（2026-08-27 双会审定案）：端点判定必须 host 后缀精确匹配、
    /// fail-closed——伪域/路径拼接/反斜杠伪装/假 scheme 壳/怪字符一律不许命中。
    #[test]
    fn xiapan_endpoint_match_is_host_suffix_exact() {
        // —— 正常端点照常命中 ——
        assert!(is_xiapan_endpoint("https://api.u-claw.org.cn"));
        assert!(is_xiapan_endpoint("https://api.u-claw.org.cn/v1"));
        assert!(is_xiapan_endpoint("https://u-claw.org"));
        assert!(is_xiapan_endpoint("http://API.U-Claw.ORG.CN:8443/x"));
        assert!(is_xiapan_endpoint("https://api.u-claw.org."), "尾点 FQDN 仍是自家");
        assert!(is_xiapan_endpoint("https://[::1]@api.u-claw.org"), "IPv6 形 user-info 在 @ 前不误伤");
        assert!(is_xiapan_endpoint("https://evil@api.u-claw.org"), "合法 user-info + 自家 host 该命中");
        assert!(is_xiapan_endpoint("https://api.u-claw.org\\evil"), "反斜杠是路径分隔，host 仍是自家");
        // —— 伪域 / 后缀 / 路径拼接 ——
        assert!(!is_xiapan_endpoint("https://fake-u-claw.org.evil.com"), "伪域后缀不许命中");
        assert!(!is_xiapan_endpoint("https://u-claw.org.evil.com"));
        assert!(!is_xiapan_endpoint("https://evil.com/u-claw.org"), "路径里的域不算");
        assert!(!is_xiapan_endpoint("https://notu-claw.org"), "非子域前缀拼接近似域不算");
        // —— user-info / 反斜杠伪装 ——
        assert!(!is_xiapan_endpoint("https://u-claw.org:443@evil.com"), "user-info 绕过不许命中");
        assert!(!is_xiapan_endpoint("https://evil.com\\@api.u-claw.org"), "反斜杠伪装的 user-info：真 host 是 evil.com");
        assert!(!is_xiapan_endpoint("https://u-claw.org@evil.com/x"), "user-info 自家 + host evil 不许命中");
        // —— 假 scheme 壳：:// 首次出现在中间 = 把 query/数组里的 URL 当 authority ——
        assert!(!is_xiapan_endpoint("https://evil.com/r?u=https://api.u-claw.org"), "query 里嵌自家 URL 不算");
        assert!(!is_xiapan_endpoint("//evil.com/r?u=https://api.u-claw.org"), "protocol-relative 假 URL 不算");
        assert!(!is_xiapan_endpoint("evil.com:8443/x#https://u-claw.org"), "fragment 里嵌自家 URL 不算");
        assert!(!is_xiapan_endpoint("evil.com\\x/https://u-claw.org.cn"), "反斜杠截断后出现的 URL 不算");
        assert!(!is_xiapan_endpoint("https:evil.com://api.u-claw.org"), "假 scheme 壳（Node 实测 host=evil.com）");
        assert!(!is_xiapan_endpoint("https:/evil.com://api.u-claw.org"), "假 scheme 壳变体");
        assert!(!is_xiapan_endpoint("custom://api.u-claw.org\\@evil.com"), "非 http/https scheme 一律拒");
        assert!(!is_xiapan_endpoint("api.u-claw.org"), "无 scheme 裸串不是合法绝对 URL");
        // —— host 字符集 / 端口闸 ——
        assert!(!is_xiapan_endpoint("https://evil.com%40api.u-claw.org/v1"), "%40 编码 @ 在 host 是 invalid");
        assert!(!is_xiapan_endpoint("https://[api.u-claw.org]"), "[] 内不是合法 IPv6 → 拒（自家域不可能是 IPv6）");
        assert!(!is_xiapan_endpoint("https://api.u-claw.org:bad"), "非数字端口非法");
        assert!(!is_xiapan_endpoint("https://api.u-claw.org:"), "空端口非法");
        assert!(!is_xiapan_endpoint("https://<api.u-claw.org"), "host 以 < 开头不是合法 URL");
        assert!(is_xiapan_endpoint("https://api.u-claw.org.cn"), "正常端点必须照常命中");
    }

    #[test]
    fn xiapan_deepseek_gets_200k_default() {
        with_sandbox("xiapan-ds", || {
            let p = builtin_providers().into_iter().find(|p| p.id == "xiapan").unwrap();
            apply_claude(&p, "sk-xp-test", None).unwrap();
            let env = read_env();
            assert_eq!(
                env.get("CLAUDE_CODE_AUTO_COMPACT_WINDOW").and_then(|v| v.as_str()),
                Some(DEEPSEEK_AUTO_COMPACT_WINDOW),
                "虾盘云 DeepSeek 应拿到 200K 压缩窗口默认"
            );
            assert_eq!(
                env.get("CLAUDE_CODE_MAX_CONTEXT_TOKENS").and_then(|v| v.as_str()),
                Some(DEEPSEEK_AUTO_COMPACT_WINDOW),
                "窗口认知纠正键应与压缩窗口同值"
            );
            assert!(env.contains_key("ANTHROPIC_BASE_URL"), "原有注入不受影响");
        });
    }

    #[test]
    fn official_claude_model_gets_no_window_keys() {
        with_sandbox("official-claude", || {
            let p = official_like("claude-sonnet-5");
            apply_claude(&p, "sk-official", None).unwrap();
            let env = read_env();
            assert!(
                !env.contains_key("CLAUDE_CODE_AUTO_COMPACT_WINDOW"),
                "官方 Claude 模型不该被我们钉压缩窗口"
            );
            assert!(!env.contains_key("CLAUDE_CODE_MAX_CONTEXT_TOKENS"));
            assert!(env.contains_key("ANTHROPIC_BASE_URL"), "端点本身照常写入");
        });
    }

    #[test]
    fn user_custom_value_wins_over_default() {
        with_sandbox("user-wins", || {
            // 用户先自己配了 500K
            let path = claude_settings_path();
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(
                &path,
                r#"{"env":{"CLAUDE_CODE_AUTO_COMPACT_WINDOW":"500000"}}"#,
            )
            .unwrap();
            let p = builtin_providers().into_iter().find(|p| p.id == "xiapan").unwrap();
            apply_claude(&p, "sk-xp-test", None).unwrap();
            let env = read_env();
            assert_eq!(
                env.get("CLAUDE_CODE_AUTO_COMPACT_WINDOW").and_then(|v| v.as_str()),
                Some("500000"),
                "用户自配的值必须原样保留（GPT 评审 C 条：让路不覆盖）"
            );
        });
    }

    #[test]
    fn reset_claude_removes_window_keys_too() {
        with_sandbox("reset", || {
            let p = builtin_providers().into_iter().find(|p| p.id == "xiapan").unwrap();
            apply_claude(&p, "sk-xp-test", None).unwrap();
            assert!(read_env().contains_key("CLAUDE_CODE_AUTO_COMPACT_WINDOW"));
            reset_claude().unwrap();
            let env = read_env();
            assert!(!env.contains_key("CLAUDE_CODE_AUTO_COMPACT_WINDOW"), "我们注入的双键，还原后不该残留");
            assert!(!env.contains_key("CLAUDE_CODE_MAX_CONTEXT_TOKENS"));
            assert!(!env.contains_key("ANTHROPIC_BASE_URL"), "还原语义不变");
        });
    }

    /// 🔴 sol 复审 P1 原始案例：用户**预先**自配且值恰好 == "200000"。
    /// 无标记 → 不是我们的注入 → 还原绝不许碰（仅凭值匹配在这里必翻车）。
    #[test]
    fn reset_spares_user_custom_value_even_when_it_equals_200000() {
        with_sandbox("reset-user-200000", || {
            let path = claude_settings_path();
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(
                &path,
                r#"{"env":{"CLAUDE_CODE_AUTO_COMPACT_WINDOW":"200000","ANTHROPIC_BASE_URL":"https://example.com"}}"#,
            )
            .unwrap();
            reset_claude().unwrap();
            let env = read_env();
            assert_eq!(
                env.get("CLAUDE_CODE_AUTO_COMPACT_WINDOW").and_then(|v| v.as_str()),
                Some("200000"),
                "无标记的用户自配（哪怕值==200000）必须原样保留"
            );
            assert!(!env.contains_key("ANTHROPIC_BASE_URL"), "其余管理键的还原语义不变");
        });
    }

    /// 用户事后手改过我们注入的值 → 尊重现状不删（删除仅限现行值仍是特征值的场合）。
    #[test]
    fn reset_spares_injected_key_user_modified_afterwards() {
        with_sandbox("reset-modified-after", || {
            let p = builtin_providers().into_iter().find(|p| p.id == "xiapan").unwrap();
            apply_claude(&p, "sk-xp-test", None).unwrap(); // 注入 + 落标记(null)
            let path = claude_settings_path();
            let mut root: Value =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            root["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"] = json!("500k");
            std::fs::write(&path, serde_json::to_string(&root).unwrap()).unwrap();

            reset_claude().unwrap();
            let env = read_env();
            assert_eq!(
                env.get("CLAUDE_CODE_AUTO_COMPACT_WINDOW").and_then(|v| v.as_str()),
                Some("500k"),
                "用户把手改了注入键 → 尊重现状"
            );
            // 另一个没被碰过的注入键照常收走
            assert!(!env.contains_key("CLAUDE_CODE_MAX_CONTEXT_TOKENS"), "未手改的注入键照常摘除");
        });
    }

    /// 我们顶掉过用户自配 → 还原时归还原值（归属标记记着「注入前有值」）。
    /// 正常流程不会出现这种状态（让路不覆盖），但标记在就按标记办——精确而非猜测。
    #[test]
    fn reset_restores_previous_user_value_when_provenance_has_one() {
        with_sandbox("restore-prev", || {
            let p = builtin_providers().into_iter().find(|p| p.id == "xiapan").unwrap();
            apply_claude(&p, "sk-xp-test", None).unwrap(); // 双键注入，标记=null
            // 模拟「标记记着旧值」的状态：A 键的标记带原值，B 键标记为 null
            let prov_path = crate::installer::user_home_dir()
                .join(".uking")
                .join("claude-env-provenance.json");
            std::fs::write(
                &prov_path,
                r#"{"CLAUDE_CODE_AUTO_COMPACT_WINDOW":"320000","CLAUDE_CODE_MAX_CONTEXT_TOKENS":null}"#,
            )
            .unwrap();

            reset_claude().unwrap();
            let env = read_env();
            assert_eq!(
                env.get("CLAUDE_CODE_AUTO_COMPACT_WINDOW").and_then(|v| v.as_str()),
                Some("320000"),
                "标记里有原值 → 归还而不是删"
            );
            assert!(!env.contains_key("CLAUDE_CODE_MAX_CONTEXT_TOKENS"), "null 标记照常摘除");
        });
    }

    #[test]
    fn delegation_env_injects_window_for_xiapan_only() {
        with_sandbox("delegation", || {
            // 干净沙箱 = 客户没配过自己的 Claude → 免配置分支生效
            let v = delegation_env("sk-del-test");
            let get = |k: &str| v.iter().find(|(key, _)| key == k).map(|(_, val)| val.clone());
            assert_eq!(get("CLAUDE_CODE_AUTO_COMPACT_WINDOW"), Some(DEEPSEEK_AUTO_COMPACT_WINDOW.to_string()));
            assert_eq!(get("CLAUDE_CODE_MAX_CONTEXT_TOKENS"), Some(DEEPSEEK_AUTO_COMPACT_WINDOW.to_string()));
            assert_eq!(get("ANTHROPIC_MODEL"), Some("deepseek-v4-flash".to_string()));
        });
    }

    #[test]
    fn delegation_env_respects_user_own_key_red_line() {
        with_sandbox("delegation-user-key", || {
            // 客户用自己的 Key（base 不含 u-claw.org）→ 整个 Claude 半边都不许注入（issue #375）
            let path = claude_settings_path();
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(
                &path,
                r#"{"env":{"ANTHROPIC_AUTH_TOKEN":"sk-theirs","ANTHROPIC_BASE_URL":"https://relay.theirs.example"}}"#,
            )
            .unwrap();
            let v = delegation_env("sk-del-test");
            assert!(
                !v.iter().any(|(k, _)| k.starts_with("ANTHROPIC")),
                "客户自己的 Key 在用时，委派 env 不许盖我们的端点/Key"
            );
            assert!(
                !v.iter().any(|(k, _)| k.starts_with("CLAUDE_CODE_")),
                "压缩窗口双键同样不许注入（红线覆盖全部 ANTHROPIC 链注入）"
            );
        });
    }

    // 🔴 sol 复审（2026-08-27）失败安全顺序的故障注入。教训：上轮初版两条测试因花括号错位
    // 嵌入别的测试函数体内部，从未被注册运行（rustc `cannot test inner items` 假绿），且
    // write_window_provenance 当时返回 () 吞错误——契约在实现里根本不存在。opus + sol 双会审
    // 一致裁定：先把写序契约做实（注入=先标记成功再写 settings；回收=先写 settings 再消费标记，
    // 任一步失败 Err、可重试）再测。下面两条同时绿才算这条契约成立。
    #[test]
    fn apply_fails_when_provenance_write_fails() {
        with_sandbox("prov-write-fail", || {
            // 把 ~/.uking 占位成文件 → write_window_provenance 的 create_dir_all 必败。
            let home = crate::installer::user_home_dir();
            std::fs::create_dir_all(&home).unwrap();
            let uking = home.join(".uking");
            std::fs::write(&uking, "file occupies the .uking slot").unwrap();

            let p = builtin_providers().into_iter().find(|p| p.id == "xiapan").unwrap();
            let r = apply_claude(&p, "sk-xp-test", None);
            assert!(r.is_err(), "标记写失败必须 Err，不许带着写不上的标记继续写 settings");
            assert!(
                !claude_settings_path().exists(),
                "注入路径上标记失败 → settings 不应被写（先标记后 settings）"
            );
        });
    }

    #[test]
    fn switching_keeps_provenance_when_settings_write_fails() {
        with_sandbox("settings-write-fail", || {
            let xp = builtin_providers().into_iter().find(|p| p.id == "xiapan").unwrap();
            apply_claude(&xp, "sk-xp-test", None).unwrap(); // 注入成功（标记已落盘）

            // settings 写路径失败注入：把 .claude 目录占位成文件 → 后续 apply 的 create_dir_all 必败。
            // （Windows 只读属性拦不住 rename 覆盖，目录占位是可靠路径。）
            let claude_dir = claude_settings_path().parent().unwrap().to_path_buf();
            std::fs::remove_dir_all(&claude_dir).unwrap();
            std::fs::write(&claude_dir, "file occupies the .claude slot").unwrap();
            let off = official_like("claude-sonnet-5");
            let r = apply_claude(&off, "sk-official", None);
            assert!(r.is_err(), "settings 写失败必须 Err");
            // 🔴 直接断言标记未被消费（opus 意见：不能靠「重试后 env 干净」间接推断）
            let prov = read_window_provenance();
            assert!(
                prov.contains_key("CLAUDE_CODE_AUTO_COMPACT_WINDOW")
                    && prov.contains_key("CLAUDE_CODE_MAX_CONTEXT_TOKENS"),
                "settings 写失败后标记必须仍在（先 settings 后消费标记）"
            );
            // 恢复可写：重建目录 + 重新注入（or_insert 幂等，标记不重复）→ 走真实还原路径 reset_claude
            std::fs::remove_file(&claude_dir).unwrap();
            apply_claude(&xp, "sk-xp-test", None).unwrap();
            reset_claude().unwrap();
            let env = read_env();
            assert!(
                !env.contains_key("CLAUDE_CODE_AUTO_COMPACT_WINDOW"),
                "settings 失败过的重试仍能回收注入键"
            );
            assert!(!env.contains_key("CLAUDE_CODE_MAX_CONTEXT_TOKENS"));
            assert!(
                !read_window_provenance().contains_key("CLAUDE_CODE_AUTO_COMPACT_WINDOW"),
                "reset 后标记应被消费"
            );
        });
    }

    // 🔴 opus 会审 P1-4（2026-08-27 实读代码）：apply 只有注入分支、没有摘除分支——
    // 客户从虾盘云 deepseek 切到 kimi/glm 或任何非 deepseek 路由时，200K 窗口键原样
    // 残留 + 标记还在，给 256K/1M 窗口的模型戴 20 万帽子（提前压缩）。设计不变量
    // 「官方模型零注入」必须在**每次 apply** 成立，不只首次。
    #[test]
    fn switching_to_non_deepseek_removes_window_keys() {
        with_sandbox("switch-nonds", || {
            let xp = builtin_providers().into_iter().find(|p| p.id == "xiapan").unwrap();
            apply_claude(&xp, "sk-xp-test", None).unwrap(); // deepseek 注入成功

            // 切到非 deepseek（官方路由）→ else 分支按标记归还/摘除窗口键
            let off = official_like("claude-sonnet-5");
            apply_claude(&off, "sk-official", None).unwrap();
            let env = read_env();
            assert!(
                !env.contains_key("CLAUDE_CODE_AUTO_COMPACT_WINDOW"),
                "切到非 deepseek 必须摘下窗口键（官方模型零注入每次 apply 成立）"
            );
            assert!(!env.contains_key("CLAUDE_CODE_MAX_CONTEXT_TOKENS"));
            assert!(
                !read_window_provenance().contains_key("CLAUDE_CODE_AUTO_COMPACT_WINDOW"),
                "归还后标记应被消费"
            );
        });
    }

    // 🔴 sol 复审（2026-08-27）P0 实锤补测：reset_claude 的写序契约 =「先落 settings、
    // 成功后才消费标记」。上版实现反了（先消费标记再写 settings）——settings 侧失败后
    // 标记丢失、重试无法回收窗口键（不可收敛）。这里注入 settings 侧失败，断言标记保留，
    // 恢复后重试仍能精确回收。
    #[test]
    fn reset_keeps_provenance_when_settings_unavailable() {
        with_sandbox("reset-settings-fail", || {
            let xp = builtin_providers().into_iter().find(|p| p.id == "xiapan").unwrap();
            apply_claude(&xp, "sk-xp-test", None).unwrap(); // settings + 标记都写好了

            // settings 侧失败注入：.claude 目录占位成文件 → reset 读不到 settings，
            // 必须静默返回且**不得消费标记**。
            let claude_dir = claude_settings_path().parent().unwrap().to_path_buf();
            std::fs::remove_dir_all(&claude_dir).unwrap();
            std::fs::write(&claude_dir, "file occupies the .claude slot").unwrap();
            reset_claude().unwrap();
            let prov = read_window_provenance();
            assert!(
                prov.contains_key("CLAUDE_CODE_AUTO_COMPACT_WINDOW")
                    && prov.contains_key("CLAUDE_CODE_MAX_CONTEXT_TOKENS"),
                "settings 侧失败时标记必须仍在（先 settings 后消费标记）"
            );

            // 恢复可写：重建目录 + 重新注入 → reset 走真实还原路径，键回收 + 标记消费
            std::fs::remove_file(&claude_dir).unwrap();
            apply_claude(&xp, "sk-xp-test", None).unwrap();
            reset_claude().unwrap();
            let env = read_env();
            assert!(
                !env.contains_key("CLAUDE_CODE_AUTO_COMPACT_WINDOW"),
                "settings 失败过的重试仍能回收注入键"
            );
            assert!(!env.contains_key("CLAUDE_CODE_MAX_CONTEXT_TOKENS"));
            assert!(
                !read_window_provenance().contains_key("CLAUDE_CODE_AUTO_COMPACT_WINDOW"),
                "reset 后标记应被消费"
            );
        });
    }
}

/// 「只有 OpenAI 端点的供应商走本地翻译桥驱动 Claude Code」这条路的钉子。
///
/// 钉两件事：**桥的 base 真写进去了**（不是悄悄回退到供应商那个不存在的 Anthropic 端点），
/// 以及**回显认得出链路里多了一环**（`claude_via_bridge`）。第二条是给客户看的：
/// 桥跟着 U-King 活，这个事实藏在一个 127.0.0.1 的 URL 里等人自己认出来 = 等于没说。
#[cfg(test)]
mod claude_bridge_tests {
    use super::*;

    fn with_sandbox(tag: &str, f: impl FnOnce()) {
        crate::testsandbox::with_sandbox(&format!("claude-bridge-{tag}"), &[".claude"], |_| f())
    }

    /// issue #359 客户机上的形状：只有 OpenAI 端点，`anthropic_base` 是 None。
    fn openai_only() -> ProviderPreset {
        ProviderPreset {
            id: "relay".into(),
            name: "某中转".into(),
            summary: String::new(),
            openai_base: "https://relay.example.com/v1".into(),
            anthropic_base: None,
            model: "gpt-x".into(),
            small_model: "gpt-x-mini".into(),
            codex_model: String::new(),
            codex_wire_api: WIRE_API.into(),
            key_url: String::new(),
            key_hint: String::new(),
            builtin_recharge: false,
            recommended: false,
            builtin: false,
            api_key: "sk-demo".into(),
        }
    }

    #[test]
    fn bridge_writes_local_base_and_status_admits_it() {
        with_sandbox("apply", || {
            let p = openai_only();
            // 不走桥：这个供应商本来就配不了 Claude Code，必须报错而不是写个坏值进去
            assert!(
                apply_claude(&p, "sk-x", None).is_err(),
                "没有 anthropic_base 还能配成功？那写进去的一定是坏值"
            );

            apply_claude_via_bridge(&p, "sk-x", None, "http://127.0.0.1:15723").unwrap();

            let s = std::fs::read_to_string(claude_settings_path()).expect("settings.json 没写出来");
            let v: Value = serde_json::from_str(&s).unwrap();
            let env = v.get("env").and_then(|e| e.as_object()).expect("env 段没了");
            assert_eq!(env["ANTHROPIC_BASE_URL"], json!("http://127.0.0.1:15723"));
            // 其余四个管理键照旧 —— 走桥只该换 base，别顺手改了模型/超时
            assert_eq!(env["ANTHROPIC_AUTH_TOKEN"], json!("sk-x"));
            assert_eq!(env["ANTHROPIC_MODEL"], json!("gpt-x"));
            assert_eq!(env["ANTHROPIC_SMALL_FAST_MODEL"], json!("gpt-x-mini"));
            assert_eq!(env["API_TIMEOUT_MS"], json!("600000"));

            // 回显必须承认「链路里多了一环」
            let st = driver_status();
            assert!(st.claude_via_bridge, "指着 127.0.0.1 却不报 via_bridge = 回显撒谎");
            assert_eq!(st.claude_base.as_deref(), Some("http://127.0.0.1:15723"));
        });
    }

    /// 直连的供应商不许被误报成走桥 —— 否则界面上人人都挂个「U-King 关了就断」的警告，
    /// 真需要警告的那台机器反而被淹掉。
    #[test]
    fn direct_provider_is_not_reported_as_bridged() {
        with_sandbox("direct", || {
            let mut p = openai_only();
            p.anthropic_base = Some("https://relay.example.com".into());
            apply_claude(&p, "sk-x", None).unwrap();
            let st = driver_status();
            assert!(!st.claude_via_bridge, "直连也报成走桥了");
        });
    }
}

/// 委派通道（`claude -p` / `codex exec` 子进程 env）的档位钉子。
///
/// 子智能体/委派是批量体力活，不该和主会话抢 pro 档 —— 曾经有人把 ANTHROPIC_MODEL
/// 单独写死成 `deepseek-v4-pro`（理由「满血才会干活」），结果跟「切驱动写进 settings.json
/// 的」漂移成两个模型，客户界面看 flash、账单却是 pro。宪法第 8 条：同一事实只一份。
/// 这两条用例把「委派模型必须 = 虾盘云 preset 的 model」钉死，不许再有人单独写死第二份。
/// ★ 测试桥（仅测试构建存在）：lib.rs 的设备钱包同步用例要复现「apply_clawx 写出的真实
/// 配置形状」再驱动真路 `sync_device_wallet_consumers`。这几个内部函数不进生产 API 面，
/// 只为测试开门 —— 别在这儿加任何新逻辑。
#[cfg(test)]
pub(crate) mod wallet_sync_test_bridge {
    pub(crate) fn builtin_xiapan() -> super::ProviderPreset {
        super::builtin_providers()
            .into_iter()
            .find(|p| p.id == "xiapan")
            .expect("内置供应商列表必有 xiapan")
    }
    pub(crate) fn managed_id(p: &super::ProviderPreset) -> String {
        super::managed_provider_id(p)
    }
    pub(crate) fn apply_clawx_as_uking(
        p: &super::ProviderPreset,
        key: &str,
        model: &str,
    ) -> Result<(), String> {
        super::apply_clawx(p, key, model)
    }
    pub(crate) fn apply_qwen_as_uking(
        p: &super::ProviderPreset,
        key: &str,
        model: &str,
    ) -> Result<(), String> {
        super::apply_qwen(p, key, model)
    }
    pub(crate) fn record_active(tool: &str, provider_id: &str) {
        super::record_active_driver(tool, provider_id)
    }
}

#[cfg(test)]
mod delegation_env_tests {
    use super::*;

    /// 🔴 **必须进沙箱**：`delegation_env` 现在会读这台机器的 Claude/Codex 配置来决定
    /// 注不注入（issue #375）。不隔离的话，用例结果取决于跑测试的人自己配了什么 ——
    /// 开发机上恰好在用自己的 Key，这几条就会红；换台机器又绿。
    fn with_sandbox(tag: &str, f: impl FnOnce()) {
        crate::testsandbox::with_sandbox(&format!("delegation-{tag}"), &[".claude", ".codex"], |_| f())
    }

    /// 委派模型必须来自虾盘云 preset，不许单独写死（更不许写死成 pro）。
    #[test]
    fn delegation_model_follows_preset_not_hardcoded_pro() {
        with_sandbox("preset", || {
        let env = delegation_env("sk-test-key");
        let get = |k: &str| {
            env.iter()
                .find(|(key, _)| key == k)
                .map(|(_, v)| v.as_str())
                .unwrap_or("")
        };
        // 单一真相源：preset 的 model 现在是 deepseek-v4-flash。
        let xiapan = builtin_providers()
            .into_iter()
            .find(|p| p.id == "xiapan")
            .expect("虾盘云 preset 必须存在");
        assert_eq!(get("ANTHROPIC_MODEL"), xiapan.model, "委派模型和 preset 漂移了");
        assert_ne!(get("ANTHROPIC_MODEL"), "deepseek-v4-pro", "委派默认不该是 pro（满血档留给主会话）");
        });
    }

    /// 委派端点/Key 也都从 preset 取：端点 = 国内可达镜像，Key = 调用方传入的那个。
    #[test]
    fn delegation_endpoint_and_key_from_preset_and_arg() {
        with_sandbox("endpoint", || {
        let env = delegation_env("sk-my-key");
        let get = |k: &str| {
            env.iter()
                .find(|(key, _)| key == k)
                .map(|(_, v)| v.as_str())
                .unwrap_or("")
        };
        let xiapan = builtin_providers()
            .into_iter()
            .find(|p| p.id == "xiapan")
            .expect("虾盘云 preset 必须存在");
        assert_eq!(get("ANTHROPIC_BASE_URL"), xiapan.anthropic_base.clone().unwrap_or_default());
        assert_eq!(get("OPENAI_BASE_URL"), xiapan.openai_base);
        assert_eq!(get("ANTHROPIC_AUTH_TOKEN"), "sk-my-key");
        assert_eq!(get("OPENAI_API_KEY"), "sk-my-key");
        // small 档同样跟 preset（省 token 的档位也归 preset 管，不另写一份）
        assert_eq!(get("ANTHROPIC_SMALL_FAST_MODEL"), xiapan.small_model);
        });
    }

    /// ★ issue #375 的回归钉子：**客户在用自己的 Key 时，委派不许把我们的 Key 盖上去**。
    ///
    /// 客户原话：「我已经选择自己的 api-key，使用中，但是还是在继续扣虾盘云的 token」。
    /// 老实现无条件全量注入 —— 他在 AI 设置里切到自己的中转、界面也如实显示着，
    /// 可只要从工作台委派一次 `claude -p`，子进程 env 就被我们盖成虾盘云 + 设备 Key，
    /// 这一轮记在我们账上，而他没做错任何事。
    #[test]
    fn own_claude_key_is_never_overridden_by_our_billing() {
        with_sandbox("own-key", || {
            // 客户自己的中转 + 自己的 Key（端点不含 u-claw.org 就算「他自己的」）
            let p = claude_settings_path();
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(
                &p,
                r#"{"env":{"ANTHROPIC_BASE_URL":"https://my-relay.example.com","ANTHROPIC_AUTH_TOKEN":"sk-他自己的"}}"#,
            )
            .unwrap();

            let env = delegation_env("sk-我们的设备key");
            let keys: Vec<&str> = env.iter().map(|(k, _)| k.as_str()).collect();
            assert!(
                !keys.contains(&"ANTHROPIC_AUTH_TOKEN"),
                "🔴 又把我们的 Key 盖到客户自己的配置上了：{keys:?}"
            );
            assert!(!keys.contains(&"ANTHROPIC_BASE_URL"), "端点也不许盖：{keys:?}");
            assert!(
                !env.iter().any(|(k, _)| k.starts_with("ANTHROPIC_") || k == "API_TIMEOUT_MS"),
                "Claude 那半边一个变量都不该有：{keys:?}"
            );
            // 两半分开判：他只切了 Claude Code，**没配过 Codex** —— Codex 那半边照旧
            // 免配置直连我们（这不是「抢」，是他本来就没有别的可用配置）。
            assert!(keys.contains(&"OPENAI_API_KEY"), "不该殃及 Codex 那半边：{keys:?}");
            assert!(keys.contains(&"OPENAI_BASE_URL"), "不该殃及 Codex 那半边：{keys:?}");
        });
    }

    /// 官方 OAuth 登录（`~/.claude/.credentials.json`）同样算「他自己的」，一样不许盖。
    #[test]
    fn official_login_is_also_left_alone() {
        with_sandbox("oauth", || {
            let cred = config_home().join(".claude").join(".credentials.json");
            std::fs::create_dir_all(cred.parent().unwrap()).unwrap();
            std::fs::write(&cred, r#"{"access_token":"官方登录的凭据占位"}"#).unwrap();
            let keys: Vec<String> = delegation_env("sk-我们的").into_iter().map(|(k, _)| k).collect();
            assert!(
                !keys.iter().any(|k| k.starts_with("ANTHROPIC_")),
                "官方登录的客户被我们抢了：{keys:?}"
            );
        });
    }
}

/// Issue #364：新版 Codex 移除了 `wire_api = "chat"`，见到它**整份 config.toml 拒绝加载**
/// → Codex 秒退（客户日志里每次都是 `code=1 total=0s events=0`），连别的 provider 和用户
/// 自己挂的 MCP 一起废掉。这组用例钉两件事：① 我们再也不会写出那个值；② 已经写坏的机器
/// 能被自愈救回来，且救的过程不许碰用户的东西。
#[cfg(test)]
mod codex_wire_api_tests {
    use super::*;

    fn with_sandbox(tag: &str, f: impl FnOnce(&std::path::Path)) {
        crate::testsandbox::with_sandbox(&format!("wireapi-{tag}"), &[".codex"], |root| {
            f(&root.join(".codex"))
        })
    }

    /// 曾经写 `chat` 的那几个内置预设（DeepSeek 官方 / 智谱 / Kimi）现在必须是 responses。
    /// 这条守的是「源头」：预设里留一个 chat，切一次驱动就又把客户机写砖一次。
    /// 断言的是「**不许出现 chat**」而不是「必须等于 responses」：
    /// `ollama`（本地大模型）和 `official`（官方直连=还原）本来就不是 Codex 供应商，
    /// 它们留空表示「不适用」，那是对的。写成「必须等于 responses」会把正确的空值判红。
    #[test]
    fn no_builtin_preset_still_declares_chat() {
        let bad: Vec<String> = builtin_providers()
            .iter()
            .filter(|p| p.codex_wire_api == "chat")
            .map(|p| p.id.clone())
            .collect();
        assert!(bad.is_empty(), "这些预设还在声明 wire_api=\"chat\"（会写砖客户的 Codex）: {bad:?}");
    }

    /// 自愈：坏配置能救活，而且**用户挂的 MCP 一个字节都不能少**。
    /// 顺带验 env_key → experimental_bearer_token 的换法（新版 Codex 对自定义 provider
    /// 不读 auth.json，留着 env_key 等于能启动但连不上）。
    #[test]
    fn heal_fixes_broken_config_and_preserves_user_mcp() {
        with_sandbox("heal", |codex| {
            std::fs::create_dir_all(codex).unwrap();
            std::fs::write(
                codex.join("auth.json"),
                r#"{"OPENAI_API_KEY":"sk-real-key-123"}"#,
            )
            .unwrap();
            let broken = "# managed by U-King —— 驱动切换写入\n\
                model = \"deepseek-v4-flash\"\n\
                model_provider = \"deepseek\"\n\n\
                [model_providers.deepseek]\n\
                name = \"DeepSeek\"\n\
                base_url = \"https://api.deepseek.com/v1\"\n\
                env_key = \"OPENAI_API_KEY\"\n\
                wire_api = \"chat\"\n\n\
                [mcp_servers.my_thing]\n\
                command = \"node\"\n\
                args = [\"C:/me/server.js\"]\n";
            let cfg = codex.join("config.toml");
            std::fs::write(&cfg, broken).unwrap();

            heal_codex_wire_api();

            let got = std::fs::read_to_string(&cfg).unwrap();
            assert!(!got.contains(r#"wire_api = "chat""#), "坏值没被修掉：{got}");
            assert!(got.contains(r#"wire_api = "responses""#), "没改成 responses：{got}");
            assert!(
                got.contains(r#"experimental_bearer_token = "sk-real-key-123""#),
                "env_key 没换成新版认的 bearer：{got}"
            );
            assert!(got.contains("[mcp_servers.my_thing]"), "把用户挂的 MCP 弄丢了：{got}");
            assert!(got.contains(r#"args = ["C:/me/server.js"]"#), "MCP 参数被改了：{got}");
        });
    }

    /// 🔴 不是我们写的文件**一个字节都不许动** —— 哪怕它里面也有 `wire_api = "chat"`。
    /// 那是客户自己配的 Codex，修不修由他决定，不由我们代劳（宪法：不碰用户真实状态）。
    #[test]
    fn foreign_config_is_never_touched() {
        with_sandbox("foreign", |codex| {
            std::fs::create_dir_all(codex).unwrap();
            let theirs = "# 客户自己写的\n[model_providers.mine]\nwire_api = \"chat\"\n";
            let cfg = codex.join("config.toml");
            std::fs::write(&cfg, theirs).unwrap();
            heal_codex_wire_api();
            assert_eq!(
                std::fs::read_to_string(&cfg).unwrap(),
                theirs,
                "动了不是我们写的 Codex 配置"
            );
        });
    }

    /// 拿不到 key 时也必须把 wire_api 修掉：能启动 > 能鉴权。
    /// 前者是全盘瘫痪，后者只是那个 provider 要重切一次。
    #[test]
    fn heals_wire_api_even_without_key() {
        with_sandbox("nokey", |codex| {
            std::fs::create_dir_all(codex).unwrap();
            let cfg = codex.join("config.toml");
            std::fs::write(
                &cfg,
                "# managed by U-King\nwire_api = \"chat\"\nenv_key = \"OPENAI_API_KEY\"\n",
            )
            .unwrap();
            heal_codex_wire_api();
            let got = std::fs::read_to_string(&cfg).unwrap();
            assert!(got.contains(r#"wire_api = "responses""#), "没救启动：{got}");
            assert!(got.contains(r#"env_key = "OPENAI_API_KEY""#), "没 key 时不该乱改鉴权行");
        });
    }
}

/// 作图路由：「虾盘云是默认，不是唯一」这句话的判据。
///
/// 这组用例守的是两件**方向相反**的事，缺一条都会出事：
///   ① 没记录时必须与解绑之前**逐字节一致** —— 绝大多数客户从不进这个设置，
///      一个只在少数人身上生效的功能不该让所有人的作图换条路。
///   ② 有记录时必须真去别人家，而且**用别人家的 Key** —— 拿设备钱包 Key 去打第三方端点
///      是把客户的钱包交出去（宪法 11 的现场版）。
#[cfg(test)]
mod draw_route_tests {
    use super::*;

    /// 造一个自定义 provider（走真实的 save_custom_provider，不手写 json —— 手写的
    /// 那份会在字段改名时静默过期，而这条链路正是我们要验的）。
    fn add_provider(id_name: &str, base: &str, key: &str) -> String {
        save_custom_provider(ProviderPreset {
            id: String::new(),
            name: id_name.into(),
            summary: String::new(),
            openai_base: base.into(),
            anthropic_base: None,
            model: String::new(),
            small_model: String::new(),
            codex_model: String::new(),
            codex_wire_api: WIRE_API.into(),
            key_url: String::new(),
            key_hint: String::new(),
            builtin_recharge: false,
            recommended: false,
            builtin: false,
            api_key: key.into(),
        })
        .expect("存自定义 provider 失败")
        .id
    }

    /// ① 没记录 → 两个常量原样。**断言写成「等于常量」而不是「等于那串字面量」**：
    /// 端点将来真要换域名时，该跟着换的是常量，不是这条用例。
    #[test]
    fn no_record_falls_back_to_builtin_urls() {
        crate::testsandbox::with_sandbox("drawroute-default", &[".uking"], |_| {
            let e = draw_endpoint();
            assert_eq!(e.gen_url, IMAGE_GEN_URL);
            assert_eq!(e.edit_url, IMAGE_EDIT_URL);
            assert!(e.api_key.is_none(), "默认路径必须回落设备钱包 Key");
            assert!(e.model.is_none(), "默认路径不许覆盖作图页选的模型");
            assert!(e.builtin);
            // Key 与模型都得原样透传（这条才是「逐字节一致」真正的判据）
            assert_eq!(e.effective_key("sk-device-wallet").unwrap(), "sk-device-wallet");
            assert_eq!(e.effective_model("seedream-4-0"), "seedream-4-0");
        });
    }

    /// 显式选虾盘云 = 和没记录一样（它是默认，不是一种特殊的自定义）。
    #[test]
    fn explicit_xiapan_is_the_same_as_no_record() {
        crate::testsandbox::with_sandbox("drawroute-xiapan", &[".uking"], |_| {
            set_draw_route(XIAPAN_ID, "gpt-image-2").unwrap();
            let e = draw_endpoint();
            assert_eq!(e.gen_url, IMAGE_GEN_URL);
            assert_eq!(e.edit_url, IMAGE_EDIT_URL);
            assert!(e.api_key.is_none());
            assert!(e.builtin);
        });
    }

    /// ② 自定义 provider → 端点拼对、Key 用它自己的、设备钱包 Key 一个字节都别想出去。
    #[test]
    fn custom_provider_uses_its_own_endpoint_and_key() {
        crate::testsandbox::with_sandbox("drawroute-custom", &[".uking"], |_| {
            let id = add_provider("我的中转站", "https://relay.example.com/v1", "sk-mine-999");
            set_draw_route(&id, "flux-pro").unwrap();

            let e = draw_endpoint();
            assert_eq!(e.gen_url, "https://relay.example.com/v1/images/generations");
            assert_eq!(e.edit_url, "https://relay.example.com/v1/images/edits");
            assert!(!e.builtin);
            assert_eq!(e.model.as_deref(), Some("flux-pro"));
            let used = e.effective_key("sk-device-wallet").unwrap();
            assert_eq!(used, "sk-mine-999");
            assert_ne!(used, "sk-device-wallet", "设备钱包 Key 泄给第三方端点了");
            assert_eq!(e.effective_model("gpt-image-2"), "flux-pro", "路由的模型该压过调用方的");
        });
    }

    /// 选了别家却没填 Key：不许悄悄回落设备钱包 Key（那才是真正危险的失败方向 ——
    /// 它会"成功出图"，钱记在我们账上、打的是别人的端点，没人会来报 bug）。
    #[test]
    fn custom_provider_without_key_errors_instead_of_borrowing_wallet() {
        crate::testsandbox::with_sandbox("drawroute-nokey", &[".uking"], |_| {
            let id = add_provider("忘了填 Key 的中转站", "https://nokey.example.com/v1", "");
            set_draw_route(&id, "flux-pro").unwrap();
            let e = draw_endpoint();
            let err = e.effective_key("sk-device-wallet").unwrap_err();
            assert!(err.contains("忘了填 Key 的中转站"), "报错得说清是哪家：{err}");
            assert!(!err.contains("sk-device-wallet"));
        });
    }

    /// ③ base 尾斜杠不许拼出双斜杠；客户把整条路径填进 base 时也不许拼出两截
    /// （`…/images/generations/images/edits` 那个形状，chat 那边实测有人这么填）。
    #[test]
    fn base_url_joining_never_doubles_slash_or_path() {
        assert_eq!(
            images_endpoint("https://a.example.com/v1/", "generations"),
            "https://a.example.com/v1/images/generations"
        );
        assert_eq!(
            images_endpoint("https://a.example.com/v1///", "edits"),
            "https://a.example.com/v1/images/edits"
        );
        assert_eq!(
            images_endpoint("https://a.example.com/v1/images/generations", "generations"),
            "https://a.example.com/v1/images/generations"
        );
        assert_eq!(
            images_endpoint("https://a.example.com/v1/images/generations", "edits"),
            "https://a.example.com/v1/images/edits"
        );

        // 走完整链路再验一次尾斜杠（上面是纯函数，这条盖的是"存进去再读回来"）
        crate::testsandbox::with_sandbox("drawroute-slash", &[".uking"], |_| {
            let id = add_provider("尾斜杠中转站", "https://slash.example.com/v1/", "sk-slash");
            set_draw_route(&id, "flux-pro").unwrap();
            let e = draw_endpoint();
            assert_eq!(e.gen_url, "https://slash.example.com/v1/images/generations");
            assert_eq!(e.edit_url, "https://slash.example.com/v1/images/edits");
            assert!(!e.gen_url.contains("//images"), "拼出双斜杠了：{}", e.gen_url);
        });
    }

    /// ④ 记录指向一个已经被删掉的 provider → 回落默认，别让作图整个哑掉。
    /// 删供应商那条路不知道作图引用了它，这条就是那个"读的时候发现没了"的兜底。
    #[test]
    fn dangling_record_falls_back_to_builtin() {
        crate::testsandbox::with_sandbox("drawroute-dangling", &[".uking"], |_| {
            let id = add_provider("待会儿就删", "https://gone.example.com/v1", "sk-gone");
            set_draw_route(&id, "flux-pro").unwrap();
            assert_eq!(draw_endpoint().provider_id, id);

            delete_custom_provider(&id).unwrap();
            let e = draw_endpoint();
            assert_eq!(e.gen_url, IMAGE_GEN_URL, "供应商没了该回落默认，不是打一个不存在的端点");
            assert_eq!(e.edit_url, IMAGE_EDIT_URL);
            assert!(e.builtin);
            assert!(e.api_key.is_none());
            assert!(e.model.is_none(), "连它的模型覆盖也得一起失效");
        });
    }

    /// 存前校验：provider 不存在、或存在但没 openai_base，一律拒 —— 让失败停在点「应用」
    /// 那一刻，别推迟到客户点「生成」（那时报错离原因已经隔了三层）。
    #[test]
    fn set_draw_route_rejects_unusable_providers() {
        crate::testsandbox::with_sandbox("drawroute-validate", &[".uking"], |_| {
            assert!(set_draw_route("", "m").is_err());
            assert!(set_draw_route("no-such-provider", "m").is_err());
            // official（官方直连=还原）只有 anthropic_base，作图打不了它
            let anth_only = builtin_providers()
                .into_iter()
                .find(|p| p.openai_base.trim().is_empty())
                .expect("内置里该有一个不带 openai_base 的（official）");
            let err = set_draw_route(&anth_only.id, "m").unwrap_err();
            assert!(err.contains("OpenAI"), "报错得说清缺的是什么：{err}");
            // 拒掉之后不许留下半截记录
            assert!(read_draw_route().provider_id.is_empty());
            assert!(draw_endpoint().builtin);
        });
    }

    /// 选了别家却不填模型 → 拒。留空看着像「用作图页选的那个」，而作图页那个下拉列的是
    /// 虾盘云的模型 id，打到别人端点上必 404 —— 一个含义是谎话的选项不如不给。
    /// 虾盘云那条**不受此限**（它的模型真相源就是作图页），这两条必须一起断言，
    /// 否则下次有人"顺手统一一下"就把默认路径也变成必填了。
    #[test]
    fn custom_route_requires_an_explicit_model_but_builtin_does_not() {
        crate::testsandbox::with_sandbox("drawroute-needmodel", &[".uking"], |_| {
            let id = add_provider("必须填模型的中转站", "https://need.example.com/v1", "sk-need");
            let err = set_draw_route(&id, "   ").unwrap_err();
            assert!(err.contains("模型"), "报错得说清缺的是什么：{err}");
            assert!(read_draw_route().provider_id.is_empty(), "拒掉了还落了盘");

            set_draw_route(XIAPAN_ID, "").expect("虾盘云不填模型是对的：它听作图页的");
            assert!(draw_endpoint().builtin);
        });
    }

    /// 回显（前端读的那个形状）不许带 Key —— 少一处泄漏面少一次事故。
    #[test]
    fn view_never_carries_api_key() {
        crate::testsandbox::with_sandbox("drawroute-view", &[".uking"], |_| {
            let id = add_provider("回显中转站", "https://view.example.com/v1", "sk-secret-abc");
            set_draw_route(&id, "flux-pro").unwrap();
            let v = draw_route_view();
            assert_eq!(v.provider_id, id);
            assert_eq!(v.provider_name, "回显中转站");
            assert_eq!(v.model, "flux-pro");
            assert!(!v.builtin);
            let json = serde_json::to_string(&v).unwrap();
            assert!(!json.contains("sk-secret-abc"), "回显把 Key 带出去了：{json}");
        });
    }

    #[test]
    fn openclaw2_route_uses_chat_model_and_fail_closed_key_order() {
        let xiapan = resolve_openai_route_for_openclaw2("xiapan", None, Some("explicit-key"), Some("device-key")).unwrap();
        assert_eq!(xiapan.model, xiapan_model(), "OpenClaw2 不得使用 codex_model");
        assert_eq!(xiapan.key_source, "explicit");
        let wallet = resolve_openai_route_for_openclaw2("xiapan", None, None, Some("device-key")).unwrap();
        assert_eq!(wallet.key_source, "device_wallet");
        assert!(resolve_openai_route_for_openclaw2("official", None, None, None).is_err());
        assert!(resolve_openai_route_for_openclaw2("deepseek", None, None, None).is_err());
        let local = resolve_openai_route_for_openclaw2("ollama", None, None, None).unwrap();
        assert_eq!(local.key_source, "loopback_placeholder");
    }
}
