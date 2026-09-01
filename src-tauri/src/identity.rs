//! U-King 的**身份**与**给 AI 的说明书**（`llms.txt`）。
//!
//! **为什么要有这个模块**：到 0.9.87 为止，U-King 的能力其实已经全是机器可读的了 ——
//! 影核 `actions::manifest()` 出 49 个动作的完整契约，`mcp serve` 能把它们原样喂给 AI，
//! `ulog` 把 17 个模块的运行日志落在 `~/.uking/logs/`。**但这些东西没有一个「入口」**：
//! 装在客户机上的 Claude Code / Codex / 任何别家 AI，**根本不知道这台机器上还站着一个
//! 能干活的 U-King**。能力齐全但发现不了，等于没有。
//!
//! 讽刺的是我们自己最清楚这件事该怎么解决 —— `geo.rs` 的网站体检里，
//! 「有没有 llms.txt」正是我们给客户网站打分的项目之一，还能帮客户**生成**一份。
//! 自己却没有。本模块补上：把 U-King 自己变成一个 AI 一眼能读懂的「本机能力层」。
//!
//! ## 唯一真相源约定（宪法第 8 条）
//!
//! **`llms.txt` 是编译产物，不是手写文档。** 它由 [`render_llms`] 从
//! `actions::manifest()` 的 JSON 现场生成 —— 加一个动作，说明书自动多一条；
//! 改一个动作的描述，说明书跟着变。**任何时候都不许手改生成出来的文件**，
//! 手改的那份第二天就和动作表漂移，而 AI 会照着漂移的那份去调不存在的动作。
//!
//! ## 明文 / 私密分离
//!
//! | 文件 | 谁能看 | 内容 |
//! |---|---|---|
//! | `~/.uking/llms.txt` | **明文**，任何 AI | 身份 + 能力目录 + 怎么调 + 日志在哪 |
//! | `~/.uking/llms-full.txt` | **明文**，任何 AI | 全量动作签名（含入参/出参 schema） |
//! | `~/.uking/identity.json` | **明文**，用户可手改 | 名字、人设、主人称呼、自定义属性 |
//! | `~/.uking/secrets.json` | **私密**，只有本机 | API Key **真值** |
//!
//! **铁律：Key 的值绝不出现在 `llms.txt` 里。** 说明书只写「配了哪些 Key、配没配」，
//! 值永远留在 `secrets.json`。这条由 [`tests::llms_never_leaks_secret_values`] 守着 ——
//! 那个测试真的把 Key 塞进去再断言渲染结果里搜不到，不是靠人自觉。
//!
//! ## 设计约束（对齐本项目的模块独立铁律）
//!
//! - **纯函数、零 `AppHandle`、零第三方运行时依赖**，`#[tauri::command]` 全在 `lib.rs`。
//! - **本模块不 import `actions`** —— manifest 由调用方（组合根 `lib.rs`）传进来。
//!   同 `metrics` 不 import `usage_local` 的手法：删掉影核也不该让本模块编不过。
//! - 所有落盘走 [`atomic_write`]：先写 `.tmp` 再 rename，中途断电不会留半个文件。

use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

/// 说明书的格式版本。**改了渲染结构就 +1** —— 下游（别家 AI、我们自己的 MCP 文档）
/// 可以靠它判断要不要重新解析。
pub const LLMS_SPEC_VERSION: &str = "1.0.0";

/// 用户没起名字时的默认身份。
const DEFAULT_NAME: &str = "U-King";

/// 用户家目录。指针要写进 `~/.claude/CLAUDE.md` 这类落点，所以对外可见。
///
/// 🔴 **复用公共层那一份，别在这儿重写** —— 这里原本自己读 `USERPROFILE`，
/// 是全仓**唯一不认 `UKING_TEST_HOME`** 的家目录实现，后果是本模块所有落点
/// （`llms.txt` / `identity.json` / `secrets.json`，以及 `identity.link` 要写的
/// **别家 AI 记忆文件**）统统逃出沙箱，一次「隔离测试」能改到用户的真实 CLAUDE.md。
pub fn home_dir() -> PathBuf {
    crate::installer::user_home_dir()
}

/// U-King 的数据根目录 `~/.uking/`。
pub fn uking_dir() -> PathBuf {
    home_dir().join(".uking")
}

/// 身份文件（明文，用户可手改）。
pub fn identity_path() -> PathBuf {
    uking_dir().join("identity.json")
}

/// 凭据文件（私密，**绝不进说明书**）。
pub fn secrets_path() -> PathBuf {
    uking_dir().join("secrets.json")
}

/// 说明书封面（明文）。
pub fn llms_path() -> PathBuf {
    uking_dir().join("llms.txt")
}

/// 说明书全量版（明文，含 schema）。
pub fn llms_full_path() -> PathBuf {
    uking_dir().join("llms-full.txt")
}

// ───────────────────────────── 身份 ─────────────────────────────

/// 用户可自定义的身份。**全部字段都可空** —— 空的就用默认值渲染，
/// 绝不因为用户没填就拒绝生成说明书。
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Identity {
    /// 这台机器上的 U-King 叫什么。用户可以改成「小海」「阿king」随便。
    #[serde(default)]
    pub name: String,
    /// 主人希望被怎么称呼。会写进说明书，让别的 AI 知道该叫用户什么。
    #[serde(default)]
    pub owner: String,
    /// 人设 / 职责定位。一句话，比如「负责海事业务文档和数据整理」。
    #[serde(default)]
    pub role: String,
    /// 自定义属性，任意键值。会原样进说明书的「属性」段。
    #[serde(default)]
    pub traits: Map<String, Value>,
    /// 主人自由补充的说明。**原样进说明书末尾** —— 这是用户对外围 AI 说话的地方，
    /// 比如「我的项目都在 D:\work，别动 C 盘」。
    #[serde(default)]
    pub notes: String,
}

impl Identity {
    /// 渲染用的显示名：空就回落到默认，不让说明书出现空标题。
    pub fn display_name(&self) -> &str {
        if self.name.trim().is_empty() { DEFAULT_NAME } else { self.name.trim() }
    }
}

/// 读身份。文件不存在或坏了都回默认值 —— **绝不因为读不出身份就让整条链失败**，
/// 说明书的价值在能力目录，身份只是封面。
pub fn load_identity() -> Identity {
    load_identity_in(&uking_dir())
}

/// 同 [`load_identity`]，但指定根目录 —— 给测试用（真跑落盘逻辑又不碰用户的 `~/.uking`）。
pub fn load_identity_in(dir: &Path) -> Identity {
    std::fs::read_to_string(dir.join("identity.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<Identity>(&s).ok())
        .unwrap_or_default()
}

/// 存身份（原子写）。
pub fn save_identity_in(dir: &Path, id: &Identity) -> Result<(), String> {
    let body = serde_json::to_vec_pretty(id).map_err(|e| format!("serialize identity: {e}"))?;
    atomic_write(&dir.join("identity.json"), &body)
}

// ───────────────────────────── 凭据 ─────────────────────────────

/// 凭据摘要：**只有名字和配没配，永远没有值**。这是给说明书用的形状。
#[derive(Debug, Clone, serde::Serialize)]
pub struct SecretSummary {
    pub name: String,
    pub configured: bool,
}

/// 读凭据表。返回 `名字 -> 值`，**只给需要真值的调用方用**（比如实际发请求）。
/// 渲染说明书一律走 [`secret_summaries`]。
pub fn load_secrets_in(dir: &Path) -> Map<String, Value> {
    std::fs::read_to_string(dir.join("secrets.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<Map<String, Value>>(&s).ok())
        .unwrap_or_default()
}

/// 凭据摘要（安全形状）。值被丢掉，只留「有这个名字」+「值非空」。
pub fn secret_summaries(secrets: &Map<String, Value>) -> Vec<SecretSummary> {
    let mut out: Vec<SecretSummary> = secrets
        .iter()
        .map(|(k, v)| SecretSummary {
            name: k.clone(),
            configured: !v.as_str().unwrap_or("").trim().is_empty(),
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// 写一条凭据（原子写）。`value` 为空串表示删除这一条。
pub fn set_secret_in(dir: &Path, name: &str, value: &str) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("凭据名不能为空".into());
    }
    let mut m = load_secrets_in(dir);
    if value.trim().is_empty() {
        m.remove(name);
    } else {
        m.insert(name.into(), Value::String(value.into()));
    }
    let body = serde_json::to_vec_pretty(&m).map_err(|e| format!("serialize secrets: {e}"))?;
    atomic_write(&dir.join("secrets.json"), &body)
}

// ───────────────────────────── 说明书渲染 ─────────────────────────────

/// 说明书正文里的平台名。
///
/// 🔴 这份文件是**给 AI 读的地图**，写错平台等于给每个上这台机器的 AI 发了张错地图。
/// 以前正文第一句硬编码「装在这台 Windows 电脑上」，在 Mac 上照样这么渲染。
fn platform_label() -> &'static str {
    if cfg!(windows) {
        "Windows"
    } else if cfg!(target_os = "macos") {
        "macOS"
    } else {
        "Linux"
    }
}

/// 说明书里教 AI 敲的那条命令 —— **必须是这台机器上真敲得动的东西**。
///
/// 🔴 以前全文 12 处硬编码 `u-king-mini.exe`。Mac 上根本没有 `.exe`，
/// 而且二进制在 `U-King.app/Contents/MacOS/` 里、**不在 PATH 上** ——
/// 任何照说明书办事的 AI，第一条命令就是 command not found。
///
/// 用 `current_exe()` 的绝对路径：说明书是机器读的，正确性比好看要紧，
/// 而且 U 盘版路径本来就随插拔变，写死任何相对形式都是错的。
fn cli_invocation() -> String {
    std::env::current_exe()
        .ok()
        .map(|p| p.display().to_string())
        .filter(|p| !p.trim().is_empty())
        .unwrap_or_else(|| {
            if cfg!(windows) { "u-king-mini.exe".into() } else { "u-king-mini".into() }
        })
}

/// 渲染 `llms.txt`（封面版）。
///
/// `manifest` 就是 `actions::manifest()` 的返回值 —— 本模块**不认识 actions**，
/// 由组合根传进来。传个空对象也能渲染（能力段会写「读不到动作表」而不是崩）。
///
/// **`secrets` 传摘要不传值**：签名上就杜绝了把 Key 渲染进去的可能，
/// 而不是靠调用方自觉。
pub fn render_llms(
    id: &Identity,
    manifest: &Value,
    secrets: &[SecretSummary],
    skills: &[(String, String)],
) -> String {
    let name = id.display_name();
    let cli = cli_invocation();
    let mut s = String::new();

    s.push_str(&format!("# {name}\n\n"));
    s.push_str(&format!(
        "> {name} 是装在这台 {plat} 电脑上的**本机 AI 能力层**。\n\
         > 你（正在读这段话的 AI）可以直接调用它来操作这台机器：查环境、切模型驱动、\n\
         > 装工具、看用量、取诊断日志。**它不是一个聊天机器人，是一组稳定的、带确认门禁的动作。**\n\n",
        plat = platform_label()
    ));
    s.push_str(&format!("说明书格式版本: {LLMS_SPEC_VERSION}（本文件由 U-King 自动生成，**请勿手改**）\n\n"));

    // ── 身份 ──
    s.push_str("## 我是谁\n\n");
    s.push_str(&format!("- 名字: {name}\n"));
    if !id.owner.trim().is_empty() {
        s.push_str(&format!("- 主人: {}（请这样称呼他/她）\n", id.owner.trim()));
    }
    if !id.role.trim().is_empty() {
        s.push_str(&format!("- 职责: {}\n", id.role.trim()));
    }
    for (k, v) in &id.traits {
        let val = v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string());
        s.push_str(&format!("- {k}: {val}\n"));
    }
    s.push('\n');

    // ── 怎么调 ──
    s.push_str("## 怎么调用我\n\n");
    s.push_str(&format!(
        "两条路，任选其一。**两条走的是同一份动作核心**，行为逐字节一致：\n\n\
         ### 1. 命令行（最省事，不用配任何东西）\n\
         ```\n\
         \"{cli}\" action list --json                 # 我能干什么\n\
         \"{cli}\" action describe <id> --json        # 某个动作的完整签名\n\
         \"{cli}\" action run <id> --json --no-input  # 跑一个只读动作\n\
         ```\n\
         **写动作必须带 `--yes`**（哪些动作要、哪些不要，见下面动作表里每条的「门禁」）：\n\
         ```\n\
         \"{cli}\" action run <id> --yes --input '<json>'\n\
         ```\n\
         输出约定：stdout **只有最终 JSON**（`| jq` 不会被污染），进度和日志一律走 stderr；\n\
         **出错时 JSON 走 stderr、stdout 是空的**。退出码 0=成功，非 0=失败。\n\n\
         ### 2. MCP（想让我常驻在你的工具表里）\n\
         ```\n\
         \"{cli}\" mcp serve              # 默认只暴露只读动作\n\
         \"{cli}\" mcp serve --allow-write # 才会出现写动作\n\
         ```\n\
         stdio + 行分隔 JSON-RPC。工具名把点号换成下划线（`runtime.driver.inspect`\n\
         → `runtime_driver_inspect`）。\n\n"
    ));

    // ── 我会哪些活（技能包）──
    // 放在动作表**之前**：客户机上的 AI 十有八九是来干活的（做份 PPT / 读份合同），
    // 不是来切驱动的。把「能出成品」的能力摆在前面，别让它先趟 55 个运维动作。
    s.push_str(&render_skills(skills));

    // ── 能力目录 ──
    s.push_str(&render_capabilities(manifest, false));

    // ── 常见任务怎么走 ──
    // **这一段才是说明书比动作清单多出来的东西**。清单是 sitemap（我能干什么），
    // 这段是 how-to（一件事该按什么顺序做、为什么不能换顺序、怎么算做成了）——
    // 网站 llms.txt 值钱的也正是后者。
    // 配方**不再从 manifest 取**：manifest 要贴着影核 0.5.0 的 schema 走
    // （`additionalProperties: false`，容不下 recipes），而配方的真源本来就是
    // `actions::recipe_list()`。从真源直接取，比经 manifest 中转还少一层。
    s.push_str(&render_recipes(&crate::actions::recipe_list()));

    // ── 日志与排障 ──
    // 排障入口。**平台相关的那两句必须跟着平台走** —— 「Windows 事件日志」和
    // 「OneDrive / Defender / PowerShell」在 Mac 上都不存在，照抄过去等于教 AI 去查空气。
    let crash_note = if cfg!(windows) {
        "不依赖 Windows 事件日志，客户机上那儿常年是空的"
    } else {
        "不依赖系统崩溃报告，U-King 自己记的那份更全"
    };
    let envfp_hotspots = if cfg!(windows) {
        "中文路径 / OneDrive / 长路径 / Defender / 代理 / PowerShell 版本"
    } else {
        "中文路径 / 路径带空格 / 代理 / locale / 各家 CLI 版本"
    };
    s.push_str(&format!(
        "## 出问题了去哪儿看\n\n\
         这台机器上**所有** U-King 动作都会留痕，你不用猜：\n\n\
         - **运行日志**: `~/.uking/logs/*.log` —— 每个模块一个文件（draw / video / clawx / geo …）\n\
         - **脱敏诊断正文**（远程排障主力，可直接贴给客服）:\n\
         \x20 `\"{cli}\" action run runtime.diagnostics.collect --json`\n\
         - **U-King 自己崩没崩**（{crash_note}）:\n\
         \x20 `\"{cli}\" action run runtime.crash.inspect --json`\n\
         - **客户装的 AI CLI 崩了还是被杀**（跟上一条分工不同）:\n\
         \x20 `\"{cli}\" action run runtime.ai_process.inspect --json`\n\
         - **环境炸点**（{envfp_hotspots}）:\n\
         \x20 `\"{cli}\" --envfp`\n\n\
         排障顺序建议：先 `--envfp` 看环境，再 `runtime.diagnostics.collect` 看发生了什么，\n\
         最后才去翻 `~/.uking/logs/` 的原文。\n\n"
    ));

    // ── 凭据 ──
    s.push_str("## 凭据\n\n");
    if secrets.is_empty() {
        s.push_str("这台机器还没配任何自定义 Key。\n");
    } else {
        s.push_str("本机配了以下 Key。**值不在本文件里**，存在 `~/.uking/secrets.json`：\n\n");
        for x in secrets {
            let mark = if x.configured { "已配" } else { "空" };
            s.push_str(&format!("- `{}` — {}\n", x.name, mark));
        }
    }
    s.push_str("\n需要用某个 Key 时读 `~/.uking/secrets.json`，**不要把值回显到对话或日志里**。\n\n");

    // ── 主人补充 ──
    if !id.notes.trim().is_empty() {
        s.push_str("## 主人的补充说明\n\n");
        s.push_str(id.notes.trim());
        s.push_str("\n\n");
    }

    s.push_str("---\n\n");
    s.push_str("更全的版本（每个动作的完整入参/出参 schema）在同目录的 `llms-full.txt`。\n");
    s
}

/// 渲染「我会哪些活」——技能包能力段。
///
/// `skills` 由组合根传进来（`(技能名, 一句话能干什么)`），本模块**不认识 skillpack**，
/// 保持模块独立铁律。空清单就整段不渲染，不留一个「暂无」的空壳。
///
/// 这段回答的是动作表回答不了的问题：动作表说的是「我能改这台机器的什么」，
/// 这段说的是「我能替你把什么活干出来」。缺一段，AI 读完只知道怎么切驱动，
/// 不知道这台机器能做 PPT。
/// 技能包**实际落在了哪几个目录** —— 现场 stat，不照着一张写死的名单说。
///
/// 🔴 以前这里硬编码四个目录 + 一句「按你是哪个 AI 挑一处，**内容完全相同**」。
/// 那句话是错的：`skillpack::skill_targets()` 给每个落点都设了门禁
/// （`~/.claude` 存在才铺、`~/.openclaw` 存在才铺、`~/.agents` 要装了 pi 才铺），
/// **没装的那家一个包都不会铺进去** —— 这个设计是对的，不给没装的工具乱扔文件。
/// 错的是说明书把四个目录说成无条件都有：本机 `~/.agents/skills` 是空的（没装 pi），
/// AI 照着说明书去那儿找，只会得出「同步坏了」的错误结论。
///
/// 不 import `skillpack`（本模块的独立性铁律），也不需要 —— 目录里有没有 `uking-*`
/// 是可以直接看的事实，比转述一份名单更不会漂。
fn render_skill_dirs() -> String {
    let home = home_dir();
    let candidates: [(&str, PathBuf); 4] = [
        ("Claude Code", home.join(".claude").join("skills")),
        ("OpenClaw / ClawX", home.join(".openclaw").join("skills")),
        ("pi 及其它遵循 Agent Skills 标准的 agent", home.join(".agents").join("skills")),
        ("U-King 自己的副本（上面几处的来源）", home.join(".uking").join("skills")),
    ];
    let count = |p: &Path| -> usize {
        std::fs::read_dir(p)
            .map(|rd| {
                rd.filter_map(Result::ok)
                    .filter(|e| {
                        e.file_name().to_string_lossy().starts_with("uking-") && e.path().is_dir()
                    })
                    .count()
            })
            .unwrap_or(0)
    };
    let mut present = String::new();
    let mut absent = String::new();
    for (label, path) in &candidates {
        let n = count(path);
        let line = format!("- `{}` — {label}", path.display());
        if n > 0 {
            present.push_str(&format!("{line}（{n} 个）\n"));
        } else {
            absent.push_str(&format!("{line}\n"));
        }
    }
    let mut s = String::new();
    if present.is_empty() {
        s.push_str("技能包**还没同步到任何一家 AI 的目录**（下面写了怎么补）：\n\n");
    } else {
        s.push_str("技能**已经铺到**这几处（内容相同，按你是哪个 AI 挑一处读）：\n\n");
        s.push_str(&present);
    }
    if !absent.is_empty() {
        s.push_str(
            "\n下面这几处**当前是空的** —— 不是坏了：对应的 AI 没装在这台机器上，\
             我们就不往它目录里扔文件。装上之后会自动补：\n\n",
        );
        s.push_str(&absent);
    }
    s.push('\n');
    s
}

fn render_skills(skills: &[(String, String)]) -> String {
    if skills.is_empty() {
        return String::new();
    }
    let mut s = String::new();
    s.push_str(&format!("## 我会哪些活（{} 个技能包，装完就在，不用联网下载）\n\n", skills.len()));
    s.push_str(
        "这些是**能出成品文件**的能力，不是聊天话术。每个技能是一个文件夹，里面有 SKILL.md（怎么干）\n\
         和现成脚本。**你直接读 SKILL.md 按它说的跑脚本即可**，不需要自己从零写实现。\n\n",
    );
    s.push_str(&render_skill_dirs());
    for (name, desc) in skills {
        s.push_str(&format!("- **`{name}`** — {desc}\n"));
    }
    let pkg_mgr = if cfg!(windows) { "winget" } else { "brew" };
    s.push_str(&format!(
        "\n**没找到这些目录？** 说明技能还没同步到你这儿，跑一次：\n\
         ```\n\
         \"{cli}\" action run runtime.skillpack.install --yes\n\
         ```\n\n\
         **脚本报缺东西（ffmpeg / LibreOffice / Python 库 / 浏览器）怎么办？**\n\
         别自己去官网下 —— U-King 管着这些外部工具，查和装都有动作：\n\
         ```\n\
         \"{cli}\" action run runtime.toolbox.inspect --json --no-input   # 有哪些、装没装\n\
         ```\n\
         装它要走 GUI 或 `install_capability_tool`（本机走 {pkg_mgr}），装完再重跑脚本即可。\n\n\
         **要花钱的能力（作图 / 视频 / 联网模型）用哪把 Key？** 脚本运行时自己读\n\
         `~/.uking/device.json` 里这台机器的内置 Key，**你不用问用户要、也不要往脚本里写 Key**。\n\
         余额不足脚本会明说，充值走 `runtime.usage_meter.inspect` 里给的入口。\n\n",
        cli = cli_invocation()
    ));
    s
}

/// 渲染 `llms-full.txt`（全量版，带 schema）。
pub fn render_llms_full(id: &Identity, manifest: &Value) -> String {
    let mut s = String::new();
    s.push_str(&format!("# {} — 全量动作签名\n\n", id.display_name()));
    s.push_str(&format!(
        "说明书格式版本: {LLMS_SPEC_VERSION}。本文件由 U-King 从动作表自动生成，**请勿手改**。\n\
         动作契约版本: {}\n\n",
        manifest.get("spec_version").and_then(Value::as_str).unwrap_or("未知")
    ));
    s.push_str(&render_capabilities(manifest, true));
    s
}

/// 能力目录。`full=true` 时带 input/output schema。
///
/// 只读和写**分开列**，因为对调用方来说这是最要紧的区别：只读的随便调，
/// 写的每一个都会改客户的机器。
/// 「常见任务怎么走」——把配方清单（`actions::recipe_list()`）渲染成 AI 读得懂的步骤。
///
/// 入参是配方数组本身，**不是 manifest**：manifest 贴着影核 0.5.0 schema，装不下 recipes。
///
/// 没有配方就**整段不渲染**，不放一个空标题：一个写着「常见任务」却底下什么都没有的小节，
/// 会让读它的 AI 以为这软件没有推荐用法，比不写更误导。
fn render_recipes(recipes: &Value) -> String {
    let Some(rs) = recipes.as_array().filter(|a| !a.is_empty()) else {
        return String::new();
    };
    let mut s = String::new();
    s.push_str(&format!("## 常见任务怎么走（{} 条配方）

", rs.len()));
    s.push_str(
        "下面每一条都是**验过的组合**，顺序是有讲究的（每步后面写了为什么在这个位置）。
照着走比你自己从动作清单里现拼更快，也更可能一次做对。

",
    );
    for r in rs {
        let g = |k: &str| r.get(k).and_then(Value::as_str).unwrap_or("");
        s.push_str(&format!("### {}

", g("title")));
        s.push_str(&format!("- **什么时候用**: {}
", g("when")));
        if let Some(pre) = r.get("preconditions").and_then(Value::as_array).filter(|a| !a.is_empty()) {
            for p in pre {
                s.push_str(&format!("- **前提**: {}
", p.as_str().unwrap_or("")));
            }
        }
        s.push_str("- **步骤**:
");
        for (i, st) in r
            .get("steps")
            .and_then(Value::as_array)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
            .iter()
            .enumerate()
        {
            let a = st.get("action").and_then(Value::as_str).unwrap_or("");
            let note = st.get("note").and_then(Value::as_str).unwrap_or("");
            s.push_str(&format!("  {}. `{}`
     {}
", i + 1, a, note));
        }
        s.push_str(&format!("- **怎么算做成了**: {}

", g("verify")));
    }
    s
}

fn render_capabilities(manifest: &Value, full: bool) -> String {
    let mut s = String::new();
    let Some(actions) = manifest.get("actions").and_then(Value::as_array) else {
        s.push_str(&format!(
            "## 我能干什么\n\n（读不到动作表 —— 跑 `\"{}\" action list --json` 现查）\n\n",
            cli_invocation()
        ));
        return s;
    };

    // 🔴 字段名照抄 manifest 的真实形状：`effects.class`（不是 `effects.effect`）。
    // 第一版我按脑子里以为的形状写成 `effects.effect`，单元测试用的又是我自己捏的 fixture，
    // 于是**测试全绿、渲染出来「0 个只读 + 50 个写」** —— 测试验的是我的假设，不是现实。
    // 现在多了一条打在真 manifest 上的测试守着（`lib.rs::llms_renders_against_the_real_manifest`）。
    let (reads, writes): (Vec<&Value>, Vec<&Value>) =
        actions.iter().partition(|a| class_of(a) == "read");

    s.push_str(&format!(
        "## 我能干什么（共 {} 个动作：{} 个只读 + {} 个写）\n\n",
        actions.len(),
        reads.len(),
        writes.len()
    ));

    s.push_str(&format!("### 只读（{} 个，随便调，不会改这台机器）\n\n", reads.len()));
    for a in &reads {
        s.push_str(&render_one(a, full));
    }

    // 🔴 **不能笼统说「写动作每一个都要人同意」** —— 实测 49 个非只读动作里有 15 个
    // `confirmation: never`（13 个 browser 页面内交互 + `runtime.origin.save` + 2 个
    // `app.imagefix.*`）。那是有意的取舍：页面内点一下不设门，真正有外部后果的
    // `browser.submit`（提交/下单/发帖）才是 `always`。
    //
    // 但**这句话正是 AI 判断「我该不该问用户」的直接依据**，说反了就会让它在该问的时候不问。
    // 所以按门禁分两组渲染，从 manifest 的真实字段取，不再由这段文案凭空断言。
    let (gated, ungated): (Vec<&&Value>, Vec<&&Value>) =
        writes.iter().partition(|a| confirmation_of(a) != "never");

    s.push_str(&format!(
        "### 写 · 要人同意（{} 个）\n\n\
         调用前必须让**人**同意：CLI 加 `--yes`，MCP 传 `confirm:true`。\n\
         没有确认就调，核心会直接返回 `confirmation_required` 挡回来 —— 门禁在入参校验之前，\n\
         机器一个字节都不会被改。\n\n",
        gated.len()
    ));
    for a in &gated {
        s.push_str(&render_one(a, full));
    }

    if !ungated.is_empty() {
        s.push_str(&format!(
            "### 写 · 不需要确认（{} 个）\n\n\
             这些动作**不会**被门禁挡（`confirmation: never`），不带 `--yes` 也会直接执行。\n\
             绝大多数是浏览器页面内的交互（点一下、填个字、翻一页）—— 它们改的是页面状态，\n\
             不是这台机器。**但真正有外部后果的 `browser.submit`（提交 / 下单 / 发帖 / 删除）\n\
             仍然是要确认的**，别拿 `browser.click` 去替代它。\n\n",
            ungated.len()
        ));
        for a in &ungated {
            s.push_str(&render_one(a, full));
        }
    }
    s
}

/// 动作的门禁。真实取值：`never` / `always`（CLI 那一侧显示成 `required`）。
/// 读不到就按最保守的来 —— 宁可多问一次人，也不要因为字段缺失就默默放行。
fn confirmation_of(a: &Value) -> &str {
    a.get("effects").and_then(|e| e.get("confirmation")).and_then(Value::as_str).unwrap_or("always")
}

/// 动作的副作用等级。真实取值：`read` / `write` / `destructive` / `external`。
fn class_of(a: &Value) -> &str {
    a.get("effects").and_then(|e| e.get("class")).and_then(Value::as_str).unwrap_or("write")
}

fn render_one(a: &Value, full: bool) -> String {
    let id = a.get("id").and_then(Value::as_str).unwrap_or("?");
    let title = a.get("title").and_then(Value::as_str).unwrap_or("");
    let desc = a.get("description").and_then(Value::as_str).unwrap_or("");
    let mut s = format!("- **`{id}`** — {title}\n");
    if !desc.is_empty() {
        s.push_str(&format!("  {desc}\n"));
    }
    // 危险性提示：destructive 的必须一眼可见，别让 AI 靠猜。
    match class_of(a) {
        "destructive" => s.push_str("  ⚠️ **破坏性**：不可逆，调之前务必让人确认清楚。\n"),
        // `external` = 会把事情交给这台机器之外（起外部进程 / 装外部程序）。
        // 对调用方来说这跟普通写不是一回事，得说出来。
        "external" => s.push_str("  ↗️ 会启动/安装本机之外的程序。\n"),
        _ => {}
    }
    if a.get("execution").and_then(|e| e.get("progress_events")).and_then(Value::as_bool) == Some(true) {
        s.push_str("  ⏳ 长任务，会持续吐进度事件（CLI 下走 stderr）。\n");
    }
    if full {
        if let Some(i) = a.get("input_schema") {
            s.push_str(&format!("  - 入参: `{i}`\n"));
        }
        if let Some(o) = a.get("output_schema") {
            s.push_str(&format!("  - 出参: `{o}`\n"));
        }
    }
    s.push('\n');
    s
}

// ─────────────────────── 发现（让别家 AI 找得到）───────────────────────
//
// **这一段才是整个模块能不能兑现的关键。** 说明书躺在 `~/.uking/llms.txt` 里，
// 没有任何 AI 会凭空想到去读它 —— 只报「文件生成了」就是本项目栽过的那个跟头：
// Token 压缩机 `installed:true` 形状全对、conformance 全绿，但 hook 改写出的裸 `rtk`
// 不在 PATH 上，客户开了两天一点没省 —— **报告是对的，世界是坏的**。
// 所以这里的 `ready` 判据是「AI 真能发现」，不是「文件存在」。

/// 指针块的起止标记。**唯一且永不改** —— `unlink` 靠它精确定位，改了就会留下删不掉的残块。
const MARK_BEGIN: &str = "<!-- BEGIN U-KING POINTER (managed by U-King; safe to delete) -->";
const MARK_END: &str = "<!-- END U-KING POINTER -->";

/// 插进各家全局记忆文件的指针正文。
///
/// **刻意写得极短**：这几行会进客户**每一个会话**的上下文，是永久成本。
/// 想说的话全放 `llms.txt` 里 —— 那份是按需读的，不占常驻上下文。
fn pointer_block() -> String {
    format!(
        "{MARK_BEGIN}\n\
         本机用户说中文：回答、解释、报错说明一律用简体中文（用户改用英文时跟着换）。\n\
         本机装有 U-King —— 一个可被 AI 直接调用的本机能力层（查环境 / 切模型驱动 / 装工具 /\n\
         看用量 / 取诊断日志）。要动这台机器前，先读 `~/.uking/llms.txt`：那里有完整能力清单、\n\
         可直接照抄的调用命令（命令行或 MCP）和日志位置，机器生成、随升级自动更新。\n\
         {MARK_END}"
    )
}

/// 一个可注入指针的落点。
pub struct LinkTarget {
    pub id: &'static str,
    pub label: &'static str,
    pub path: PathBuf,
    /// 这个路径存在才认为「这台机器上有这个 harness」。
    gate: PathBuf,
    /// 目标文件不存在时允不允许新建。`~/AGENTS.md` 是 false ——
    /// 不在客户家目录根上凭空造文件，那是垃圾。
    create_if_missing: bool,
}

/// 各家 harness 的全局记忆文件。**只挑真的读这些文件的**，别乱撒。
pub fn link_targets_in(home: &Path) -> Vec<LinkTarget> {
    vec![
        LinkTarget {
            id: "claude",
            label: "Claude Code",
            path: home.join(".claude").join("CLAUDE.md"),
            gate: home.join(".claude"),
            create_if_missing: true,
        },
        LinkTarget {
            id: "codex",
            label: "Codex",
            path: home.join(".codex").join("AGENTS.md"),
            gate: home.join(".codex"),
            create_if_missing: true,
        },
        LinkTarget {
            // 跨 harness 通用（Codex / Kimi Code 等都读它）。**只在它已经存在时才动** ——
            // 客户家目录根上有没有这个文件是他自己的事，我们不替他决定。
            id: "agents",
            label: "AGENTS.md（跨工具通用）",
            path: home.join("AGENTS.md"),
            gate: home.join("AGENTS.md"),
            create_if_missing: false,
        },
    ]
}

/// 每个落点的现状：这台机器上**有没有这个 harness**、**指针挂上了没**。
pub fn discovery_in(home: &Path) -> Vec<Value> {
    link_targets_in(home)
        .into_iter()
        .map(|t| {
            let eligible = t.gate.exists();
            let body = std::fs::read_to_string(&t.path).unwrap_or_default();
            let linked = body.contains(MARK_BEGIN);
            json!({
                "id": t.id,
                "label": t.label,
                "path": t.path.display().to_string(),
                "eligible": eligible,
                "linked": linked,
                "reason": if eligible { Value::Null } else { json!(format!("这台机器上没装 {}", t.label)) },
            })
        })
        .collect()
}

/// 把指针写进指定落点（幂等）。`ids` 为空 = 写进所有 eligible 的落点。
///
/// **只增不删**：用户原有内容一个字节都不动，我们的东西全在标记块里；
/// 已经有块就整块替换（这样升级后指针文案能刷新，而不是越堆越多）。
/// 首次改动前留底 `*.uking-bak`，对齐 providers.rs 的既有约定。
pub fn link_in(home: &Path, ids: &[String]) -> Result<Vec<String>, String> {
    let mut done = Vec::new();
    for t in link_targets_in(home) {
        if !ids.is_empty() && !ids.iter().any(|x| x == t.id) {
            continue;
        }
        if !t.gate.exists() {
            continue;
        }
        let existed = t.path.exists();
        if !existed && !t.create_if_missing {
            continue;
        }
        let old = std::fs::read_to_string(&t.path).unwrap_or_default();
        // 首次改动留底：只在我们**还没碰过**且文件本来就有内容时备份。
        let bak = t.path.with_extension("md.uking-bak");
        if existed && !old.contains(MARK_BEGIN) && !bak.exists() && !old.trim().is_empty() {
            let _ = std::fs::write(&bak, old.as_bytes());
        }
        let next = upsert_block(&old, &pointer_block());
        atomic_write(&t.path, next.as_bytes())?;
        done.push(t.path.display().to_string());
    }
    Ok(done)
}

/// 撤销：**只删我们那一块**，用户原有内容原样留下。
pub fn unlink_in(home: &Path) -> Result<Vec<String>, String> {
    let mut done = Vec::new();
    for t in link_targets_in(home) {
        let Ok(old) = std::fs::read_to_string(&t.path) else { continue };
        if !old.contains(MARK_BEGIN) {
            continue;
        }
        let next = remove_block(&old);
        atomic_write(&t.path, next.as_bytes())?;
        done.push(t.path.display().to_string());
    }
    Ok(done)
}

/// 有块就换掉，没有就追加到末尾。
fn upsert_block(old: &str, block: &str) -> String {
    match (old.find(MARK_BEGIN), old.find(MARK_END)) {
        (Some(a), Some(b)) if b > a => {
            let mut s = String::with_capacity(old.len() + block.len());
            s.push_str(&old[..a]);
            s.push_str(block);
            s.push_str(&old[b + MARK_END.len()..]);
            s
        }
        _ => {
            let mut s = old.to_string();
            if !s.is_empty() && !s.ends_with('\n') {
                s.push('\n');
            }
            if !s.is_empty() {
                s.push('\n');
            }
            s.push_str(block);
            s.push('\n');
            s
        }
    }
}

/// 精确摘掉块，顺手收掉它留下的连续空行（免得反复 link/unlink 把文件撑出一堆空行）。
fn remove_block(old: &str) -> String {
    let (Some(a), Some(b)) = (old.find(MARK_BEGIN), old.find(MARK_END)) else {
        return old.to_string();
    };
    if b < a {
        return old.to_string();
    }
    let mut s = String::with_capacity(old.len());
    s.push_str(old[..a].trim_end_matches(['\n', '\r']));
    let tail = &old[b + MARK_END.len()..];
    let tail = tail.trim_start_matches(['\n', '\r']);
    if !tail.is_empty() {
        s.push_str("\n\n");
        s.push_str(tail);
    } else if !s.is_empty() {
        s.push('\n');
    }
    s
}

// ───────────────────────────── 发布 ─────────────────────────────

/// 生成并落盘两份说明书。返回写了哪些文件。
///
/// **幂等**：内容一样就重复写也没关系（原子写，不会写坏）。
pub fn publish_in(
    dir: &Path,
    id: &Identity,
    manifest: &Value,
    skills: &[(String, String)],
) -> Result<Vec<String>, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    let secrets = secret_summaries(&load_secrets_in(dir));

    let cover = render_llms(id, manifest, &secrets, skills);
    let full = render_llms_full(id, manifest);

    atomic_write(&dir.join("llms.txt"), cover.as_bytes())?;
    atomic_write(&dir.join("llms-full.txt"), full.as_bytes())?;

    Ok(vec![
        dir.join("llms.txt").display().to_string(),
        dir.join("llms-full.txt").display().to_string(),
    ])
}

/// 只读状态：身份 + 说明书在哪 + 发布了没 + **AI 到底能不能发现** + 凭据摘要。
///
/// 🔴 **`ready` 的判据是「AI 真能发现」，不是「文件生成了」。**
/// 这两件事差得远：`llms.txt` 静静躺在 `~/.uking/` 里，没有任何 AI 会凭空去读它。
/// 只报前者就是本项目栽过的那个跟头 —— Token 压缩机 `installed:true` 形状全对、
/// conformance 全绿，而客户开了两天一点没省，因为改写出的裸 `rtk` 不在 PATH 上。
/// **报告是对的，世界是坏的。** 所以这里必须两个条件都满足才算 ready。
pub fn inspect_in(dir: &Path, home: &Path) -> Value {
    let id = load_identity_in(dir);
    let secrets = secret_summaries(&load_secrets_in(dir));
    let cover = dir.join("llms.txt");
    let full = dir.join("llms-full.txt");
    let published = cover.exists() && full.exists();

    let discovery = discovery_in(home);
    let linked_n = discovery.iter().filter(|d| d["linked"] == json!(true)).count();
    let eligible_n = discovery.iter().filter(|d| d["eligible"] == json!(true)).count();

    let mut blockers: Vec<String> = Vec::new();
    if !published {
        blockers.push("说明书还没生成 —— 跑 identity.publish".into());
    }
    if linked_n == 0 {
        // 措辞不能预设说明书已经生成 —— 两条 blocker 会同时出现，
        // 说「说明书生成了，但…」跟上一条直接打架，客户不知道该信哪句。
        blockers.push(if eligible_n == 0 {
            "这台机器上没发现任何 AI 工具（Claude Code / Codex），没有可挂指针的地方".into()
        } else {
            format!(
                "{eligible_n} 个 AI 工具的记忆文件里都没有指向说明书的指针 —— \
                 它们不会自己想到去读 ~/.uking/llms.txt。跑 identity.link 挂上"
            )
        });
    }

    json!({
        "spec_version": LLMS_SPEC_VERSION,
        "ready": published && linked_n > 0,
        "blockers": blockers,
        "discovery": discovery,
        "linked_count": linked_n,
        "identity": {
            "name": id.display_name(),
            "owner": id.owner,
            "role": id.role,
            "traits": id.traits,
            "notes": id.notes,
        },
        "files": {
            "identity": dir.join("identity.json").display().to_string(),
            "secrets": dir.join("secrets.json").display().to_string(),
            "llms": cover.display().to_string(),
            "llms_full": full.display().to_string(),
            "logs_dir": dir.join("logs").display().to_string(),
        },
        "published": published,
        "secrets": secrets,
    })
}

// ───────────────────────────── 公共 ─────────────────────────────

/// 原子写：先写同目录的 `.tmp` 再 rename。中途断电只会留个 tmp，
/// 绝不会让用户拿到半个文件（宪法第 10 条）。
fn atomic_write(path: &Path, data: &[u8]) -> Result<(), String> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).map_err(|e| format!("create {}: {e}", p.display()))?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, data).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("rename -> {}: {e}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sandbox(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("uking-identity-test-{tag}"));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// 🔴 **沙箱必须兜得住本模块的所有落点。**
    ///
    /// 2026-08-08 实测：本模块的 `home_dir()` 曾是全仓唯一不认 `UKING_TEST_HOME` 的
    /// 家目录实现，于是 `runtime.identity.publish` 在沙箱下**写了真实的
    /// `~/.uking/llms.txt`**；同一个 `home_dir()` 还喂着 `identity.link`，
    /// 那个动作会写 `~/.claude/CLAUDE.md` 这类**别家 AI 的记忆文件** ——
    /// 一次「隔离测试」就能改到用户真实的配置。
    ///
    /// 逃逸不会报错，只会安静地写到真机上，所以必须有一条用例专门钉住它。
    #[test]
    fn all_paths_stay_inside_the_sandbox() {
        let sb = crate::testsandbox::enter("identity-escape", &[]);
        let root = sb.root();
        for (name, p) in [
            ("home_dir", home_dir()),
            ("uking_dir", uking_dir()),
            ("identity_path", identity_path()),
            ("secrets_path", secrets_path()),
            ("llms_path", llms_path()),
            ("llms_full_path", llms_full_path()),
        ] {
            assert!(
                p.starts_with(root),
                "{name} 逃出沙箱：{} 不在 {} 里 —— 写动作会落到用户真实机器上",
                p.display(),
                root.display()
            );
        }
    }

    /// 🔴 **字段名必须和 `actions::manifest()` 真实输出一致** —— `effects.class`、
    /// `execution.progress_events`。第一版这里是我凭印象捏的（`effects.effect`），
    /// 结果测试全绿而真实渲染出「0 个只读 + 50 个写」。捏 fixture 就要捏对，
    /// 否则测的是自己的想象；真正的保险在 `lib.rs::llms_renders_against_the_real_manifest`。
    fn fake_manifest() -> Value {
        json!({
            "spec_version": "0.5.0",
            "actions": [
                {
                    "id": "runtime.stack.inspect",
                    "title": "Inspect toolchain",
                    "description": "Read node/npm/git versions.",
                    "effects": { "class": "read", "confirmation": "never", "risk": "low", "reversible": true },
                    "execution": { "progress_events": false, "idempotent": true, "timeout_ms": 30000 },
                    "input_schema": { "type": "object" },
                    "output_schema": { "type": "object" }
                },
                {
                    "id": "provider.delete",
                    "title": "Delete a provider",
                    "description": "Remove a saved provider.",
                    "effects": { "class": "destructive", "confirmation": "always", "risk": "high", "reversible": false },
                    "execution": { "progress_events": false, "idempotent": true, "timeout_ms": 10000 },
                    "input_schema": { "type": "object" },
                    "output_schema": { "type": "object" }
                },
                {
                    "id": "backup.create",
                    "title": "Create backup",
                    "description": "Snapshot to USB.",
                    "effects": { "class": "write", "confirmation": "always", "risk": "medium", "reversible": true },
                    "execution": { "progress_events": true, "idempotent": true, "timeout_ms": 600000 },
                    "input_schema": { "type": "object" },
                    "output_schema": { "type": "object" }
                }
            ]
        })
    }

    /// 🔴 这条是本模块存在的理由之一：**Key 的值绝不许出现在明文说明书里**。
    /// 不靠调用方自觉 —— 真把 Key 写进 secrets.json，再断言渲染结果里搜不到。
    #[test]
    fn llms_never_leaks_secret_values() {
        let dir = sandbox("leak");
        let poison = "sk-xp-THIS-MUST-NEVER-APPEAR-IN-LLMS";
        set_secret_in(&dir, "xiapan", poison).unwrap();
        set_secret_in(&dir, "openai", "sk-openai-ALSO-SECRET").unwrap();

        let id = Identity { name: "小海".into(), ..Default::default() };
        let files = publish_in(&dir, &id, &fake_manifest(), &[]).unwrap();
        assert_eq!(files.len(), 2);

        for f in ["llms.txt", "llms-full.txt"] {
            let body = std::fs::read_to_string(dir.join(f)).unwrap();
            assert!(!body.contains(poison), "{f} 泄漏了 Key 真值!");
            assert!(!body.contains("sk-openai-ALSO-SECRET"), "{f} 泄漏了 Key 真值!");
        }
        // 但**名字**要在，否则 AI 不知道这台机器有哪些凭据可用。
        let cover = std::fs::read_to_string(dir.join("llms.txt")).unwrap();
        assert!(cover.contains("xiapan"), "凭据名该出现在说明书里");
        assert!(cover.contains("openai"));
    }

    /// 说明书是**从 manifest 编译**出来的，不是手写的 —— 动作表里有什么，
    /// 说明书里就该有什么。这条守着「同一事实只有一份」。
    #[test]
    fn capabilities_come_from_the_manifest_not_a_hand_written_list() {
        let dir = sandbox("compile");
        let id = Identity::default();
        publish_in(&dir, &id, &fake_manifest(), &[]).unwrap();
        let cover = std::fs::read_to_string(dir.join("llms.txt")).unwrap();

        assert!(cover.contains("runtime.stack.inspect"));
        assert!(cover.contains("provider.delete"));
        assert!(cover.contains("backup.create"));
        // 分类计数要对：1 只读 + 2 写
        assert!(cover.contains("共 3 个动作：1 个只读 + 2 个写"), "实际:\n{cover}");
        // 破坏性动作必须打标，别让 AI 靠猜
        assert!(cover.contains("⚠️ **破坏性**"));
        // 长任务要标出来
        assert!(cover.contains("⏳ 长任务"));
    }

    /// 动作表读不到时**降级但不崩** —— 说明书的封面（身份、日志入口）依然有用。
    #[test]
    fn renders_without_a_manifest() {
        let id = Identity { name: "阿K".into(), ..Default::default() };
        let s = render_llms(&id, &json!({}), &[], &[]);
        assert!(s.contains("阿K"));
        assert!(s.contains("读不到动作表"));
        assert!(s.contains("~/.uking/logs/"), "日志入口任何时候都该在");
    }

    /// 身份空着也要能渲染，不能因为用户没填就出个空标题。
    #[test]
    fn empty_identity_falls_back_to_default_name() {
        let id = Identity::default();
        assert_eq!(id.display_name(), "U-King");
        let s = render_llms(&id, &json!({}), &[], &[]);
        assert!(s.starts_with("# U-King"));
    }

    /// 身份存了要能原样读回来（round-trip），否则界面上改完刷新就没了。
    #[test]
    fn identity_round_trips() {
        let dir = sandbox("roundtrip");
        let mut traits = Map::new();
        traits.insert("常用语言".into(), json!("中文"));
        let id = Identity {
            name: "小海".into(),
            owner: "李工".into(),
            role: "海事业务文档整理".into(),
            traits,
            notes: "项目都在 D:\\work，别动 C 盘".into(),
        };
        save_identity_in(&dir, &id).unwrap();

        let back = load_identity_in(&dir);
        assert_eq!(back.name, "小海");
        assert_eq!(back.owner, "李工");
        assert_eq!(back.role, "海事业务文档整理");
        assert_eq!(back.traits.get("常用语言").unwrap(), "中文");
        assert!(back.notes.contains("别动 C 盘"));

        // 主人的补充说明要真的进说明书 —— 那是用户对外围 AI 说话的唯一通道
        let s = render_llms(&back, &json!({}), &[], &[]);
        assert!(s.contains("别动 C 盘"));
        assert!(s.contains("李工"));
    }

    /// 身份文件坏了不该让整条链失败 —— 回默认值继续。
    #[test]
    fn corrupt_identity_degrades_to_default() {
        let dir = sandbox("corrupt");
        std::fs::write(dir.join("identity.json"), b"{ this is not json").unwrap();
        assert_eq!(load_identity_in(&dir).display_name(), "U-King");
    }

    /// 造一个「家目录」沙箱：`home/.claude/`、`home/.codex/` 按需建。
    fn home_sandbox(tag: &str, with: &[&str]) -> PathBuf {
        let h = std::env::temp_dir().join(format!("uking-identity-home-{tag}"));
        let _ = std::fs::remove_dir_all(&h);
        std::fs::create_dir_all(h.join(".uking")).unwrap();
        for w in with {
            std::fs::create_dir_all(h.join(format!(".{w}"))).unwrap();
        }
        h
    }

    /// 🔴 **本模块最核心的一条**：`ready` 必须等于「AI 真能发现」，
    /// 不能等于「文件生成了」。只满足前半边就报 ready = 重演 Token 压缩机那个跟头
    /// （`installed:true` 形状全对，客户开两天一点没省）。
    #[test]
    fn ready_needs_real_discovery_not_just_a_generated_file() {
        let h = home_sandbox("ready", &["claude"]);
        let u = h.join(".uking");

        // ① 什么都没有
        assert_eq!(inspect_in(&u, &h)["ready"], false);

        // ② 说明书生成了 —— 但没人指向它，**依然不算 ready**
        publish_in(&u, &Identity::default(), &fake_manifest(), &[]).unwrap();
        let mid = inspect_in(&u, &h);
        assert_eq!(mid["ready"], false, "文件存在 ≠ AI 能发现，不许报 ready");
        let b = mid["blockers"].as_array().unwrap();
        assert!(
            b.iter().any(|x| x.as_str().unwrap_or("").contains("不会自己想到去读")),
            "blocker 得说清差的是「指针」而不是「文件」: {b:?}"
        );

        // ③ 指针挂上 → 才算 ready
        link_in(&h, &[]).unwrap();
        let after = inspect_in(&u, &h);
        assert_eq!(after["ready"], true);
        assert!(after["blockers"].as_array().unwrap().is_empty());
        assert_eq!(after["linked_count"], 1);
    }

    /// 🔴 **只增不删**：用户在 CLAUDE.md 里写的东西，一个字节都不许动。
    #[test]
    fn link_never_touches_user_content() {
        let h = home_sandbox("preserve", &["claude"]);
        let f = h.join(".claude").join("CLAUDE.md");
        let mine = "# 我自己的全局规则\n\n- 别动 C 盘\n- 提交前必须跑测试\n";
        std::fs::write(&f, mine).unwrap();

        link_in(&h, &[]).unwrap();
        let after = std::fs::read_to_string(&f).unwrap();
        assert!(after.contains("# 我自己的全局规则"), "用户内容被吃了");
        assert!(after.contains("- 提交前必须跑测试"));
        assert!(after.contains(MARK_BEGIN));
        // 首次改动要留底
        assert!(h.join(".claude").join("CLAUDE.md.uking-bak").exists(), "没留备份");

        // 撤销后必须**逐字节回到原样**
        unlink_in(&h).unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), mine, "unlink 没能还原原文");
    }

    /// 幂等：反复 link 不能越堆越多块。
    #[test]
    fn link_is_idempotent() {
        let h = home_sandbox("idem", &["claude"]);
        for _ in 0..3 {
            link_in(&h, &[]).unwrap();
        }
        let body = std::fs::read_to_string(h.join(".claude").join("CLAUDE.md")).unwrap();
        assert_eq!(body.matches(MARK_BEGIN).count(), 1, "指针块被插了多次");
        assert_eq!(body.matches(MARK_END).count(), 1);
    }

    /// 没装的工具不碰 —— 不在客户机上凭空造 `.codex/` 这种目录。
    #[test]
    fn link_skips_tools_that_are_not_installed() {
        let h = home_sandbox("skip", &["claude"]); // 只有 claude
        let done = link_in(&h, &[]).unwrap();
        assert_eq!(done.len(), 1);
        assert!(!h.join(".codex").exists(), "不该给没装 Codex 的机器造 .codex 目录");
        assert!(!h.join("AGENTS.md").exists(), "不该在家目录根上凭空造 AGENTS.md");

        let d = discovery_in(&h);
        let codex = d.iter().find(|x| x["id"] == "codex").unwrap();
        assert_eq!(codex["eligible"], false);
        assert!(codex["reason"].as_str().unwrap().contains("没装"));
    }

    /// `~/AGENTS.md` 已经存在时才写 —— 这是跨工具通用文件，存在与否是客户的事。
    #[test]
    fn agents_md_is_updated_only_when_it_already_exists() {
        let h = home_sandbox("agents", &[]);
        assert!(link_in(&h, &[]).unwrap().is_empty(), "什么都没装时不该写任何文件");

        std::fs::write(h.join("AGENTS.md"), "# 我的跨工具约定\n").unwrap();
        let done = link_in(&h, &[]).unwrap();
        assert_eq!(done.len(), 1);
        let body = std::fs::read_to_string(h.join("AGENTS.md")).unwrap();
        assert!(body.contains("# 我的跨工具约定") && body.contains(MARK_BEGIN));
    }

    /// 指针正文进的是**每一个会话**的上下文，是永久成本 —— 必须短。
    #[test]
    fn pointer_block_stays_small() {
        let n = pointer_block().chars().count();
        assert!(n < 400, "指针块 {n} 字，太长了：它会进客户每一个会话的上下文");
        assert!(pointer_block().contains("~/.uking/llms.txt"), "指针得指向说明书");
    }

    /// 空值删除一条凭据；摘要里 configured 要如实反映。
    #[test]
    fn secret_set_and_delete() {
        let dir = sandbox("secrets");
        set_secret_in(&dir, "xiapan", "sk-1").unwrap();
        let s = secret_summaries(&load_secrets_in(&dir));
        assert_eq!(s.len(), 1);
        assert!(s[0].configured);

        set_secret_in(&dir, "xiapan", "").unwrap();
        assert!(secret_summaries(&load_secrets_in(&dir)).is_empty());

        assert!(set_secret_in(&dir, "  ", "x").is_err(), "空名字该被拒");
    }
}
