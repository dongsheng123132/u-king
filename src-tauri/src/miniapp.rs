//! U-King 小程序运行时 —— 清单 / 安装 / 卸载 / 动作分发 / 资源服务 / 权限。
//!
//! 规范：ActionParity MiniApp Profile 0.1（`action-parity/miniapp@0.1`）。
//! 一个小程序 = 一份清单 + 一个 GUI 外壳 + 一组 ActionParity 动作；
//! 人点图标、AI 无头调用、影核远端调用走的是同一个 `actions::run`。
//!
//! **独立可插拔**：纯 std + serde_json + flate2/tar（都已是依赖），不 import 任何业务模块。
//! AI / 文件 / 图像等宿主能力经 `HostBridge` trait 注入 —— 本模块**没有网络代码，
//! 也从不读 device.json**，所以它不可能泄露 Key（拿不到的东西泄不了）。
//!
//! 删它：lib.rs 去 `mod miniapp;` + 协议注册 + handler 条目；actions.rs 去 2 行委派。

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const PROFILE: &str = "action-parity/miniapp@0.1";

/// 写清单时用的规范版本（脚手架、自检夹具用它）。
pub const SPEC_VERSION: &str = "0.5.0";

/// 这份清单的规范版本能不能吃。
///
/// 不再维护一张精确列表 —— ActionParity 一天之内从 0.1.0 走到 0.5.0，
/// 每次都要改宿主追一遍，等于**上游一发版，用户已装的小程序全废**，
/// 而第三方作者根本不知道是谁弄坏的。
///
/// 判断依据换成实际的兼容语义：迄今每一次变更都是**加字段**（顶层必填从没动过），
/// 而消费方只读自己认识的字段，多出来的字段天然无害。真正会破坏的是删字段/改必填 ——
/// 那种事该走大版本。所以：0.x 一律收，1.0 之后再谈。
fn spec_version_ok(v: &str) -> bool {
    let mut it = v.split('.');
    match (it.next().and_then(|s| s.parse::<u32>().ok()), it.next()) {
        (Some(0), Some(_)) => true,
        _ => false,
    }
}

// ───────────────────────────── 路径 ─────────────────────────────

fn uking_home() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".uking")
}

pub fn apps_root() -> PathBuf {
    // 测试用 UKING_APPS_ROOT 顶掉，保证 --miniapp-test 绝不碰真实 ~/.uking
    if let Ok(p) = std::env::var("UKING_APPS_ROOT") {
        return PathBuf::from(p);
    }
    uking_home().join("apps")
}


/// 小程序自己的沙箱。**故意放在 <app-id>/ 之外**：重装/升级整目录替换，用户数据不跟着没。
pub fn data_dir(id: &str) -> PathBuf {
    apps_root().join(".data").join(id)
}

fn staging_root() -> PathBuf {
    apps_root().join(".staging")
}
fn trash_root() -> PathBuf {
    apps_root().join(".trash")
}
fn registry_path() -> PathBuf {
    apps_root().join("registry.json")
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// ───────────────────────────── 清单 ─────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct AppIdent {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub min_host_version: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct WebPkg {
    #[serde(default = "def_web_root")]
    pub root: String,
    #[serde(default = "def_web_entry")]
    pub entry: String,
    /// 动作模块（不碰 DOM）。任何 headless 动作都要靠它 —— 没有它 parity 就是空话。
    #[serde(default)]
    pub actions: Option<String>,
}
fn def_web_root() -> String {
    "web".into()
}
fn def_web_entry() -> String {
    "index.html".into()
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ScriptPkg {
    #[serde(default)]
    pub skill_dir: String,
    #[serde(default)]
    pub runtime: String,
    #[serde(default)]
    pub entry: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct NativePkg {
    #[serde(default)]
    pub exe: String,
    #[serde(default)]
    pub action_cli: Option<String>,
    #[serde(default)]
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Package {
    pub kind: String, // web | script | native
    #[serde(default)]
    pub web: Option<WebPkg>,
    #[serde(default)]
    pub script: Option<ScriptPkg>,
    #[serde(default)]
    pub native: Option<NativePkg>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct QuickAction {
    pub action: String,
    pub label: String,
    #[serde(default)]
    pub icon: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Ui {
    pub icon: String,
    #[serde(default)]
    pub accent: Option<String>,
    #[serde(default = "def_container")]
    pub container: String,
    #[serde(default)]
    pub window: Option<Value>,
    #[serde(default = "def_true")]
    pub home_dock: bool,
    #[serde(default)]
    pub quick_actions: Vec<QuickAction>,
}
fn def_container() -> String {
    "embed".into()
}
fn def_true() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct AiPerms {
    #[serde(default)]
    pub image_generate: bool,
    #[serde(default)]
    pub image_edit: bool,
    #[serde(default)]
    pub chat: bool,
    #[serde(default)]
    pub video_generate: bool,
    #[serde(default = "def_max_calls")]
    pub max_calls_per_run: u32,
}
fn def_max_calls() -> u32 {
    4
}

#[derive(Debug, Clone, Deserialize)]
pub struct FsPerms {
    #[serde(default = "def_true")]
    pub app_data: bool,
    #[serde(default)]
    pub save_dialog: bool,
    #[serde(default)]
    pub open_dialog: bool,
}
impl Default for FsPerms {
    fn default() -> Self {
        Self { app_data: true, save_dialog: false, open_dialog: false }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct NetPerms {
    #[serde(default)]
    pub allow: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Perms {
    #[serde(default)]
    pub ai: AiPerms,
    #[serde(default)]
    pub fs: FsPerms,
    #[serde(default)]
    pub net: NetPerms,
    #[serde(default)]
    pub host_actions: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub profile: String,
    pub app: AppIdent,
    #[serde(default)]
    pub action_parity: Option<String>,
    pub package: Package,
    pub ui: Ui,
    #[serde(default)]
    pub permissions: Perms,
}

/// 前端卡片用的轻量信息（不含 web 载荷）。
#[derive(Debug, Clone, Serialize)]
pub struct MiniAppInfo {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub version: String,
    pub summary: Option<String>,
    pub kind: String,
    pub icon: String,
    pub accent: Option<String>,
    pub container: String,
    pub home_dock: bool,
    pub pinned_home: bool,
    pub enabled: bool,
    pub actions: Vec<String>,
    pub quick_actions: Vec<Value>,
    pub permissions: Vec<String>,
    pub dev: bool,
}

// ───────────────────────────── 注册表 ─────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct RegEntry {
    id: String,
    #[serde(default = "def_true")]
    enabled: bool,
    #[serde(default = "def_true")]
    pinned_home: bool,
    #[serde(default)]
    installed_at: i64,
    #[serde(default)]
    source: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct Registry {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    apps: Vec<RegEntry>,
    /// 用户**主动删掉**的内置小程序 id（墓碑）。
    ///
    /// 🔴 没有这个字段，删除按钮就是骗人的：`bundled_apps::ensure_installed` 每次启动都跑，
    /// 卸载后 `get()` 返 Err → 它判成「首次安装」→ 原样装回来。用户看到的是「我删了，
    /// 重启又在」，比功能坏更伤信任 —— 它意味着**你的决定没被记住**。
    /// 重新装（补装内置 / 从文件装）会清掉对应墓碑，所以这不是单向门。
    #[serde(default)]
    removed: Vec<String>,
}

fn read_registry() -> Registry {
    std::fs::read_to_string(registry_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_registry(r: &Registry) {
    let _ = std::fs::create_dir_all(apps_root());
    if let Ok(s) = serde_json::to_string_pretty(r) {
        let _ = std::fs::write(registry_path(), s);
    }
}

// ───────────────────────────── 校验 ─────────────────────────────

fn valid_app_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
        && !id.starts_with('.')
}

fn valid_slug(s: &str) -> bool {
    let b = s.as_bytes();
    (2..=24).contains(&b.len())
        && b[0].is_ascii_lowercase()
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

/// 动作 ID 形如 app.<slug>.<域>.<动词>
fn valid_action_id(id: &str, slug: &str) -> bool {
    let Some(rest) = id.strip_prefix(&format!("app.{slug}.")) else {
        return false;
    };
    let segs: Vec<&str> = rest.split('.').filter(|s| !s.is_empty()).collect();
    segs.len() >= 2
        && segs.iter().all(|s| {
            let b = s.as_bytes();
            b[0].is_ascii_lowercase()
                && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_' || *c == b'-')
        })
}

/// 读一个已解包的目录，做完整校验。宿主不信任打包方，安装和预览都要过这一关。
pub fn load_dir(dir: &Path) -> Result<(Manifest, Value), String> {
    let mpath = dir.join("uking-app.json");
    let text = std::fs::read_to_string(&mpath).map_err(|e| format!("读不到 uking-app.json: {e}"))?;
    let m: Manifest = serde_json::from_str(&text).map_err(|e| format!("uking-app.json 解析失败: {e}"))?;

    if m.profile != PROFILE {
        return Err(format!("profile 应为 {PROFILE}，实际 {}", m.profile));
    }
    if !valid_app_id(&m.app.id) {
        return Err(format!("非法 app.id: {}", m.app.id));
    }
    if !valid_slug(&m.app.slug) {
        return Err(format!("非法 app.slug: {}（只允许小写字母/数字/连字符，2-24 字符，禁下划线）", m.app.slug));
    }

    let rel = m.action_parity.clone().unwrap_or_else(|| "./action-parity.json".into());
    let ppath = safe_join(dir, rel.trim_start_matches("./")).ok_or("action_parity 路径非法")?;
    let ptext = std::fs::read_to_string(&ppath).map_err(|e| format!("读不到 {rel}: {e}"))?;
    let parity: Value = serde_json::from_str(&ptext).map_err(|e| format!("{rel} 解析失败: {e}"))?;

    // 认一组版本，而不是死钉一个。
    //
    // 教训：ActionParity 在一个开发周期内从 0.1.0 跳到 0.3.0（0.2.0 拆开了「声明的」与
    // 「有证据的」parity，0.3.0 加了 headless_evidence / reachability 两个可选字段）。
    // 原来这里写死 == "0.1.0"，等于**上游每跳一次版本，所有已装的第三方小程序全部失效** ——
    // 而第三方根本不知道自己被谁弄坏了。
    //
    // 迄今的变更都是加法（必填字段一个没动），所以老清单在新版下依然结构合法，
    // 宿主该照单全收。真遇到不认识的版本，给一句人能看懂的话，别只说「不合法」。
    let sv = parity.get("spec_version").and_then(|v| v.as_str()).unwrap_or("");
    if !spec_version_ok(sv) {
        return Err(format!(
            "这个小程序用的是 ActionParity {sv}，当前 U-King 只吃 0.x —— 升级 U-King 试试"
        ));
    }
    let pid = parity.pointer("/application/id").and_then(|v| v.as_str()).unwrap_or("");
    if pid != m.app.id {
        return Err(format!("身份不一致: uking-app.json={} action-parity.json={pid}", m.app.id));
    }
    let pver = parity.pointer("/application/version").and_then(|v| v.as_str()).unwrap_or("");
    if pver != m.app.version {
        return Err(format!("版本不一致: uking-app.json={} action-parity.json={pver}", m.app.version));
    }

    let acts = parity.get("actions").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    if acts.is_empty() {
        return Err("action-parity.json 里一个动作都没有".into());
    }
    let mut any_headless = false;
    for a in &acts {
        let id = a.get("id").and_then(|v| v.as_str()).unwrap_or("");
        if !valid_action_id(id, &m.app.slug) {
            return Err(format!("动作 \"{id}\" 不在命名空间 app.{}. 内", m.app.slug));
        }
        if a.pointer("/execution/headless").and_then(|v| v.as_bool()) == Some(true) {
            any_headless = true;
        }
    }

    // 无头实现必须真实存在，否则 CLI/MCP/影核那三个面全是空头支票
    match m.package.kind.as_str() {
        "web" => {
            let w = m.package.web.clone().ok_or("package.kind=web 但缺 package.web")?;
            let root = safe_join(dir, &w.root).ok_or("package.web.root 路径非法")?;
            if !safe_join(&root, &w.entry).map(|p| p.exists()).unwrap_or(false) {
                return Err(format!("入口文件不存在: {}/{}", w.root, w.entry));
            }
            if any_headless {
                let am = w.actions.clone().ok_or(
                    "有动作声明 headless=true，但 package.web.actions 没填 —— 无头调用时没有实现可跑",
                )?;
                if !safe_join(&root, &am).map(|p| p.exists()).unwrap_or(false) {
                    return Err(format!("动作模块不存在: {}/{am}", w.root));
                }
            }
        }
        "script" => {
            let s = m.package.script.clone().ok_or("package.kind=script 但缺 package.script")?;
            if !safe_join(dir, &s.skill_dir).map(|p| p.exists()).unwrap_or(false) {
                return Err(format!("skill 目录不存在: {}", s.skill_dir));
            }
        }
        "native" => {
            let n = m.package.native.clone().ok_or("package.kind=native 但缺 package.native")?;
            if !safe_join(dir, &n.exe).map(|p| p.exists()).unwrap_or(false) {
                return Err(format!("可执行文件不存在: {}", n.exe));
            }
        }
        other => return Err(format!("不认识的 package.kind: {other}")),
    }

    if !m.ui.icon.starts_with("lucide:")
        && !safe_join(dir, &m.ui.icon).map(|p| p.exists()).unwrap_or(false)
    {
        return Err(format!("图标文件不存在: {}", m.ui.icon));
    }

    // 引用完整性
    let ids: Vec<&str> = acts.iter().filter_map(|a| a.get("id").and_then(|v| v.as_str())).collect();
    for q in &m.ui.quick_actions {
        if !ids.contains(&q.action.as_str()) {
            return Err(format!("ui.quick_actions 引用了不存在的动作 {}", q.action));
        }
    }
    for h in &m.permissions.host_actions {
        if h.starts_with("app.") {
            return Err(format!("permissions.host_actions 里的 {h} 是小程序动作，这里只填宿主动作"));
        }
        // 🔴 **装的时候就拦**，别等运行时才发现（同一条规矩在 `dispatch_capability` 里
        // 还有一道 —— 那道防的是有人手改注册表/清单绕过安装）。
        // 装机期拒绝的好处是：作者当场知道自己写错了，而不是发出去之后用户点一下才炸。
        match crate::actions::describe(h) {
            Ok(spec) if spec.effect == "read" => {}
            Ok(spec) => {
                return Err(format!(
                    "permissions.host_actions 里的 {h} 是「{}」类动作 —— 小程序只能调只读动作。",
                    spec.effect
                ))
            }
            Err(_) => return Err(format!("permissions.host_actions 里的 {h} 不在动作表里")),
        }
    }

    if let Some(min) = &m.app.min_host_version {
        if version_lt(env!("CARGO_PKG_VERSION"), min) {
            return Err(format!(
                "这个小程序需要 U-King {min} 或更新，当前是 {}",
                env!("CARGO_PKG_VERSION")
            ));
        }
    }

    Ok((m, parity))
}

fn version_lt(a: &str, b: &str) -> bool {
    let p = |s: &str| -> Vec<u32> {
        s.split(['.', '-', '+'])
            .take(3)
            .map(|x| x.parse().unwrap_or(0))
            .collect()
    };
    let (x, y) = (p(a), p(b));
    for i in 0..3 {
        let (u, v) = (*x.get(i).unwrap_or(&0), *y.get(i).unwrap_or(&0));
        if u != v {
            return u < v;
        }
    }
    false
}

/// 把相对路径安全地拼到 base 下。任何越界（.. / 绝对路径 / 盘符 / 符号链接）一律 None。
/// 这是资源服务和解包共用的那道门 —— 路径穿越就是从这里漏出去的。
pub fn safe_join(base: &Path, rel: &str) -> Option<PathBuf> {
    if rel.contains('\0') {
        return None;
    }
    let mut out = base.to_path_buf();
    for seg in rel.split(['/', '\\']) {
        match seg {
            "" | "." => continue,
            ".." => return None,
            s => {
                if s.contains(':') || is_reserved_name(s) {
                    return None;
                }
                out.push(s);
            }
        }
    }
    // 最终路径必须仍在 base 之内（防符号链接绕过）
    let cb = base.canonicalize().ok()?;
    match out.canonicalize() {
        Ok(co) => co.starts_with(&cb).then_some(out),
        // 还不存在的路径（写入场景）：靠上面的逐段检查兜底
        Err(_) => Some(out),
    }
}

fn is_reserved_name(s: &str) -> bool {
    let stem = s.split('.').next().unwrap_or(s).to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit())
}

// ───────────────────────────── 查询 ─────────────────────────────

fn perms_summary(p: &Perms) -> Vec<String> {
    let mut v = vec![];
    if p.ai.image_edit {
        v.push("修改图片（消耗额度）".into());
    }
    if p.ai.image_generate {
        v.push("生成图片（消耗额度）".into());
    }
    if p.ai.chat {
        v.push("调用对话模型（消耗额度）".into());
    }
    if p.ai.video_generate {
        v.push("生成视频（消耗额度）".into());
    }
    if p.fs.save_dialog {
        v.push("保存文件到你选的位置".into());
    }
    if p.fs.open_dialog {
        v.push("打开你选的文件".into());
    }
    for h in &p.net.allow {
        v.push(format!("访问 {h}"));
    }
    for h in &p.host_actions {
        v.push(format!("调用宿主动作 {h}"));
    }
    v
}

fn info_of(dir: &Path, reg: &Registry, dev: bool) -> Option<MiniAppInfo> {
    let (m, parity) = load_dir(dir).ok()?;
    let e = reg.apps.iter().find(|e| e.id == m.app.id);
    let actions = parity
        .get("actions")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.get("id").and_then(|v| v.as_str()).map(String::from))
                .collect()
        })
        .unwrap_or_default();
    Some(MiniAppInfo {
        id: m.app.id.clone(),
        slug: m.app.slug.clone(),
        name: m.app.name.clone(),
        version: m.app.version.clone(),
        summary: m.app.summary.clone(),
        kind: m.package.kind.clone(),
        icon: m.ui.icon.clone(),
        accent: m.ui.accent.clone(),
        container: m.ui.container.clone(),
        home_dock: m.ui.home_dock,
        pinned_home: e.map(|e| e.pinned_home).unwrap_or(m.ui.home_dock),
        enabled: e.map(|e| e.enabled).unwrap_or(true),
        actions,
        quick_actions: m
            .ui
            .quick_actions
            .iter()
            .map(|q| json!({ "action": q.action, "label": q.label, "icon": q.icon }))
            .collect(),
        permissions: perms_summary(&m.permissions),
        dev,
    })
}

/// 已装小程序列表。**自愈**：盘上有清单但注册表里没有的自动补录，注册表里有但目录没了的丢弃。
/// 真相是各 app 目录里的清单，注册表只是索引/排序缓存。
pub fn list() -> Vec<MiniAppInfo> {
    let root = apps_root();
    let mut reg = read_registry();
    let mut out = vec![];
    let mut seen = vec![];

    if let Ok(rd) = std::fs::read_dir(&root) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || !e.path().is_dir() {
                continue;
            }
            if let Some(i) = info_of(&e.path(), &reg, false) {
                seen.push(i.id.clone());
                out.push(i);
            }
        }
    }
    // 开发态（脚手架产物）也列出来，但打上 dev 标记
    if let Ok(rd) = std::fs::read_dir(root.join(".dev")) {
        for e in rd.flatten() {
            if e.path().is_dir() {
                if let Some(mut i) = info_of(&e.path(), &reg, true) {
                    i.pinned_home = false;
                    seen.push(i.id.clone());
                    out.push(i);
                }
            }
        }
    }

    let before = reg.apps.len();
    reg.apps.retain(|a| seen.contains(&a.id));
    for i in &out {
        if !i.dev && !reg.apps.iter().any(|a| a.id == i.id) {
            reg.apps.push(RegEntry {
                id: i.id.clone(),
                enabled: true,
                pinned_home: i.home_dock,
                installed_at: now_ms(),
                source: "adopted".into(),
            });
        }
    }
    if reg.apps.len() != before {
        write_registry(&reg);
    }

    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

pub fn get(id: &str) -> Result<MiniAppInfo, String> {
    let dir = resolve_dir(id).ok_or_else(|| format!("没装这个小程序: {id}"))?;
    let dev = dir.parent().map(|p| p.ends_with(".dev")).unwrap_or(false);
    info_of(&dir, &read_registry(), dev).ok_or_else(|| format!("清单读不出来: {id}"))
}

/// 正式安装目录优先，其次开发态目录。
fn resolve_dir(id: &str) -> Option<PathBuf> {
    if !valid_app_id(id) {
        return None;
    }
    let a = apps_root().join(id);
    if a.join("uking-app.json").exists() {
        return Some(a);
    }
    let d = apps_root().join(".dev").join(id);
    d.join("uking-app.json").exists().then_some(d)
}

pub fn manifest(id: &str) -> Result<Value, String> {
    let dir = resolve_dir(id).ok_or_else(|| format!("没装这个小程序: {id}"))?;
    Ok(load_dir(&dir)?.1)
}

pub fn permissions(id: &str) -> Result<Perms, String> {
    let dir = resolve_dir(id).ok_or_else(|| format!("没装这个小程序: {id}"))?;
    Ok(load_dir(&dir)?.0.permissions)
}

// ───────────────────────────── 资源服务 ─────────────────────────────

pub struct Served {
    pub status: u16,
    pub mime: &'static str,
    pub body: Vec<u8>,
    /// 开发态不缓存，保存刷新即见
    pub no_store: bool,
}

fn mime_of(p: &str) -> &'static str {
    match p.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "md" | "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn err_page(status: u16, msg: &str) -> Served {
    Served {
        status,
        mime: "text/html; charset=utf-8",
        body: format!(
            "<!doctype html><meta charset=utf-8><body style=\"font:14px/1.6 system-ui;background:#14161a;color:#fca5a5;padding:32px\">\
             <h2 style=\"margin:0 0 8px\">小程序打不开</h2><pre style=\"white-space:pre-wrap;color:#e6e8ec\">{}</pre></body>",
            msg.replace('<', "&lt;")
        )
        .into_bytes(),
        no_store: true,
    }
}

/// 服务 `<app-id>/<web.root>/<rel>`。清单校验失败时返回可读的红字页面，而不是白屏。
pub fn serve(app_id: &str, rel: &str) -> Served {
    let Some(dir) = resolve_dir(app_id) else {
        return err_page(404, &format!("没装这个小程序: {app_id}"));
    };
    let (m, _) = match load_dir(&dir) {
        Ok(v) => v,
        Err(e) => return err_page(500, &e),
    };
    let dev = dir.parent().map(|p| p.ends_with(".dev")).unwrap_or(false);

    // 图标走独立虚路径，前端 <img src="uking://localhost/app/<id>/__icon">
    if rel == "__icon" {
        if m.ui.icon.starts_with("lucide:") {
            return err_page(404, "该小程序使用内置图标");
        }
        return match safe_join(&dir, &m.ui.icon).and_then(|p| std::fs::read(p).ok()) {
            Some(b) => Served { status: 200, mime: mime_of(&m.ui.icon), body: b, no_store: dev },
            None => err_page(404, "图标读不到"),
        };
    }

    let w = m.package.web.clone().unwrap_or_default();
    if m.package.kind != "web" {
        return err_page(400, "这个小程序不是 web 形态，没有可服务的页面");
    }
    let Some(root) = safe_join(&dir, &w.root) else {
        return err_page(500, "package.web.root 路径非法");
    };
    let rel = if rel.is_empty() { w.entry.as_str() } else { rel };
    let Some(file) = safe_join(&root, rel) else {
        return err_page(403, "路径非法");
    };
    match std::fs::read(&file) {
        Ok(b) => Served { status: 200, mime: mime_of(rel), body: b, no_store: dev },
        Err(_) => err_page(404, &format!("找不到 {rel}")),
    }
}

/// 给入口 HTML 注入桥脚本。
/// **小程序自己不写 <script src=bridge>** —— 由宿主注入，AI 生成的小程序才不会漏写，
/// 内嵌 iframe 和独立窗口两种容器也不会分叉。
pub fn inject_bridge(html: &[u8], app_id: &str) -> Vec<u8> {
    let tag = format!(
        "<script src=\"/__uking/bridge.js\" data-app=\"{}\"></script>",
        app_id.replace('"', "")
    );
    let s = String::from_utf8_lossy(html).to_string();
    let lower = s.to_ascii_lowercase();
    let out = if let Some(i) = lower.find("<head>") {
        let at = i + "<head>".len();
        format!("{}{}{}", &s[..at], tag, &s[at..])
    } else if let Some(i) = lower.find("<html") {
        let at = s[i..].find('>').map(|k| i + k + 1).unwrap_or(0);
        format!("{}{}{}", &s[..at], tag, &s[at..])
    } else {
        format!("{tag}{s}")
    };
    out.into_bytes()
}

/// 下发给小程序文档的 CSP。`connect-src 'self'` 是承重墙：
/// 挡住恶意小程序把用户的图偷偷发去第三方。net.allow 获准的源才追加进去。
pub fn csp_for(p: &Perms) -> String {
    let mut connect = String::from("'self'");
    for o in &p.net.allow {
        if o.starts_with("https://") {
            connect.push(' ');
            connect.push_str(o.trim_end_matches('/'));
        }
    }
    format!(
        "default-src 'self'; img-src 'self' data: blob:; media-src 'self' data: blob:; \
         style-src 'self' 'unsafe-inline'; script-src 'self' 'unsafe-inline'; \
         connect-src {connect}; object-src 'none'; base-uri 'none'; frame-ancestors 'self'"
    )
}

// ───────────────────────────── 权限 ─────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cap {
    AiImageEdit,
    AiImageGenerate,
    AiChat,
    AiVideoGenerate,
    FsAppData,
    FsSaveDialog,
    FsOpenDialog,
}

pub fn permits(app_id: &str, cap: Cap) -> bool {
    let Ok(p) = permissions(app_id) else { return false };
    match cap {
        Cap::AiImageEdit => p.ai.image_edit,
        Cap::AiImageGenerate => p.ai.image_generate,
        Cap::AiChat => p.ai.chat,
        Cap::AiVideoGenerate => p.ai.video_generate,
        Cap::FsAppData => p.fs.app_data,
        Cap::FsSaveDialog => p.fs.save_dialog,
        Cap::FsOpenDialog => p.fs.open_dialog,
    }
}

// ───────────────────────────── 动作投影 ─────────────────────────────

/// 把所有已装小程序的动作摊平成 ActionSpec，喂给 actions::list()。
/// 于是 `U-King.exe action list` 一条命令就能看到宿主动作 + 全部小程序动作。
///
/// 🔴 **投影时强制收紧门禁：非只读的小程序动作一律 `confirmation: required`**，
/// 不管它自己在 manifest 里写的是什么（见 [`projected_confirmation`]）。
pub fn action_specs() -> Vec<crate::actions::ActionSpec> {
    let mut out = vec![];
    for i in list() {
        if !i.enabled {
            continue;
        }
        let Ok(parity) = manifest(&i.id) else { continue };
        let Some(acts) = parity.get("actions").and_then(|v| v.as_array()) else { continue };
        for a in acts {
            let Some(id) = a.get("id").and_then(|v| v.as_str()) else { continue };
            out.push(crate::actions::ActionSpec {
                id: id.to_string(),
                title: a.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                description: a.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                effect: a.pointer("/effects/class").and_then(|v| v.as_str()).unwrap_or("external").to_string(),
                confirmation: projected_confirmation(a),
                idempotent: a.pointer("/execution/idempotent").and_then(|v| v.as_bool()).unwrap_or(false),
                progress_events: a.pointer("/execution/progress_events").and_then(|v| v.as_bool()).unwrap_or(false),
                timeout_ms: a
                    .pointer("/execution/timeout_ms")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(600_000),
                input_schema: a.get("input_schema").cloned(),
                output_schema: a.get("output_schema").cloned(),
                bindings: a.get("bindings").cloned(),
                // 小程序动作暂不做观测记账（它们是单来源动作，不聚合）。
                observes: &[],
            });
        }
    }
    out
}

/// 投影时对小程序动作的确认门禁做**强制收紧**：
///
/// - 非只读（`/effects/class` 不是 `read`）的动作，一律投影成 `confirmation: required`，
///   不管它在 manifest 里自己写的是 `never` 还是别的。小程序 manifest 是装软件时带的
///   数据，不该由它决定「要不要确认」；`app.*` 动作会改这台机器，门禁按宿主核心的标准
///   走（入参 `confirm: true` / CLI `--yes`）。
/// - 只读动作照抄 manifest（缺省 `never`）—— 读不改机，不设门禁。
///
/// 这一层是**投影时**收紧：真正拦在 `actions.rs::run_inner` 的 `confirmation_required`
/// 门前，CLI / GUI / MCP 走到宿主核心统一被拦，不用每个调用面各写一遍。
fn projected_confirmation(a: &Value) -> String {
    let effect = a
        .pointer("/effects/class")
        .and_then(|v| v.as_str())
        .unwrap_or("external");
    if effect != "read" {
        return "required".to_string();
    }
    a.pointer("/effects/confirmation")
        .and_then(|v| v.as_str())
        .unwrap_or("never")
        .to_string()
}

/// 由动作 ID 反查它属于哪个小程序。
pub fn owner_of(action_id: &str) -> Option<String> {
    let slug = action_id.strip_prefix("app.")?.split('.').next()?;
    list().into_iter().find(|i| i.slug == slug).map(|i| i.id)
}

// ───────────────────────────── 入参校验 ─────────────────────────────

/// 按动作自己声明的 input_schema 校验入参。
/// 拒绝非法输入才是把动作交给 agent 时的安全前提 —— agent 会瞎试，schema 是唯一的护栏。
/// 覆盖 type/required/additionalProperties/enum/数值边界/字符串长度，够用且零依赖。
pub fn validate_input(schema: &Value, input: &Value, at: &str) -> Result<(), String> {
    let bad = |m: String| Err(format!("invalid_input: {m}"));

    if let Some(t) = schema.get("type").and_then(|v| v.as_str()) {
        let ok = match t {
            "object" => input.is_object(),
            "array" => input.is_array(),
            "string" => input.is_string(),
            "number" => input.is_number(),
            "integer" => input.is_i64() || input.is_u64(),
            "boolean" => input.is_boolean(),
            "null" => input.is_null(),
            _ => true,
        };
        if !ok {
            return bad(format!("{at} 类型应为 {t}"));
        }
    }
    if let Some(e) = schema.get("enum").and_then(|v| v.as_array()) {
        if !e.contains(input) {
            return bad(format!("{at} 必须是 {}", serde_json::to_string(e).unwrap_or_default()));
        }
    }
    if let Some(s) = input.as_str() {
        if let Some(n) = schema.get("minLength").and_then(|v| v.as_u64()) {
            if (s.chars().count() as u64) < n {
                return bad(format!("{at} 至少 {n} 字"));
            }
        }
        if let Some(n) = schema.get("maxLength").and_then(|v| v.as_u64()) {
            if (s.chars().count() as u64) > n {
                return bad(format!("{at} 最多 {n} 字"));
            }
        }
    }
    if let Some(x) = input.as_f64() {
        for (k, ok) in [
            ("minimum", x >= schema.get("minimum").and_then(|v| v.as_f64()).unwrap_or(f64::MIN)),
            ("maximum", x <= schema.get("maximum").and_then(|v| v.as_f64()).unwrap_or(f64::MAX)),
            ("exclusiveMinimum", x > schema.get("exclusiveMinimum").and_then(|v| v.as_f64()).unwrap_or(f64::MIN)),
            ("exclusiveMaximum", x < schema.get("exclusiveMaximum").and_then(|v| v.as_f64()).unwrap_or(f64::MAX)),
        ] {
            if schema.get(k).is_some() && !ok {
                return bad(format!("{at} 越界（{k}）"));
            }
        }
    }
    if let Some(o) = input.as_object() {
        if let Some(req) = schema.get("required").and_then(|v| v.as_array()) {
            for r in req.iter().filter_map(|v| v.as_str()) {
                if !o.contains_key(r) {
                    return bad(format!("缺少必填字段 {at}.{r}"));
                }
            }
        }
        let props = schema.get("properties").and_then(|v| v.as_object());
        if schema.get("additionalProperties").and_then(|v| v.as_bool()) == Some(false) {
            for k in o.keys() {
                if !props.map(|p| p.contains_key(k)).unwrap_or(false) {
                    return bad(format!("不认识的字段 {at}.{k}"));
                }
            }
        }
        if let Some(p) = props {
            for (k, v) in o {
                if let Some(s) = p.get(k) {
                    validate_input(s, v, &format!("{at}.{k}"))?;
                }
            }
        }
    }
    if let (Some(arr), Some(items)) = (input.as_array(), schema.get("items")) {
        for (i, v) in arr.iter().enumerate() {
            validate_input(items, v, &format!("{at}[{i}]"))?;
        }
    }
    Ok(())
}

// ───────────────────────────── 安装 / 卸载 ─────────────────────────────

const MAX_ENTRIES: usize = 2000;
const MAX_TOTAL: u64 = 64 * 1024 * 1024;
const MAX_ENTRY: u64 = 8 * 1024 * 1024;

fn ext_allowed(kind: &str, name: &str) -> bool {
    let e = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    let base: &[&str] = &[
        "html", "htm", "js", "mjs", "css", "json", "png", "jpg", "jpeg", "webp", "gif", "svg",
        "woff2", "ttf", "md", "txt",
    ];
    if base.contains(&e.as_str()) {
        return true;
    }
    match kind {
        "script" => ["py", "sh", "mjs", "cjs"].contains(&e.as_str()),
        "native" => ["exe", "dll", "so", "dylib"].contains(&e.as_str()),
        _ => false,
    }
}

/// 解包 .ukapp（tar.gz）到 staging。
/// 加固不可省：手写归档处理是供应链 CVE 的老巢。
fn extract_targz(bytes: &[u8], dest: &Path) -> Result<(), String> {
    use std::io::Read;
    let gz = flate2::read::GzDecoder::new(bytes);
    let mut ar = tar::Archive::new(gz);
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;

    let mut n = 0usize;
    let mut total = 0u64;
    for entry in ar.entries().map_err(|e| format!("归档读取失败: {e}"))? {
        let mut e = entry.map_err(|e| format!("归档条目损坏: {e}"))?;
        n += 1;
        if n > MAX_ENTRIES {
            return Err(format!("条目过多（上限 {MAX_ENTRIES}）"));
        }
        let kind = e.header().entry_type();
        if kind.is_symlink() || kind.is_hard_link() {
            return Err("包里含符号/硬链接，拒绝安装".into());
        }
        let path = e.path().map_err(|_| "条目路径非法".to_string())?.to_path_buf();
        let rel = path.to_string_lossy().replace('\\', "/");
        if rel.starts_with('/') || rel.contains("..") || rel.contains(':') {
            return Err(format!("条目路径越界: {rel}"));
        }
        let Some(out) = safe_join(dest, &rel) else {
            return Err(format!("条目路径非法: {rel}"));
        };
        if kind.is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            continue;
        }
        if !kind.is_file() {
            continue;
        }
        let size = e.header().size().unwrap_or(0);
        if size > MAX_ENTRY {
            return Err(format!("单个文件过大: {rel}"));
        }
        total += size;
        if total > MAX_TOTAL {
            return Err(format!("解压总量超限（上限 {}MB）", MAX_TOTAL / 1024 / 1024));
        }
        if let Some(p) = out.parent() {
            std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
        let mut buf = Vec::with_capacity(size as usize);
        e.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        std::fs::write(&out, &buf).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn copy_tree(from: &Path, to: &Path, kind: &str, budget: &mut (usize, u64)) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| e.to_string())?;
    for e in std::fs::read_dir(from).map_err(|e| e.to_string())?.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "node_modules" {
            continue;
        }
        let ft = e.file_type().map_err(|e| e.to_string())?;
        if ft.is_symlink() {
            return Err(format!("拒绝符号链接: {name}"));
        }
        let src = e.path();
        let dst = to.join(&name);
        if ft.is_dir() {
            copy_tree(&src, &dst, kind, budget)?;
        } else {
            budget.0 += 1;
            if budget.0 > MAX_ENTRIES {
                return Err("文件过多".into());
            }
            let sz = e.metadata().map(|m| m.len()).unwrap_or(0);
            if sz > MAX_ENTRY {
                return Err(format!("文件过大: {name}"));
            }
            budget.1 += sz;
            if budget.1 > MAX_TOTAL {
                return Err("总量超限".into());
            }
            if !ext_allowed(kind, &name) && name != "uking-app.json" && name != "action-parity.json" {
                return Err(format!("不允许的文件类型: {name}"));
            }
            std::fs::copy(&src, &dst).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// 从目录或 .ukapp 安装。校验通过后原子换入（同卷 rename）。
pub fn install_from_path(src: &Path, source_label: &str) -> Result<MiniAppInfo, String> {
    let root = apps_root();
    std::fs::create_dir_all(staging_root()).map_err(|e| e.to_string())?;
    let stage = staging_root().join(format!("s{}", now_ms()));
    let _ = std::fs::remove_dir_all(&stage);

    if src.is_dir() {
        // 先读一遍清单拿 kind（扩展名白名单要按 kind 放宽）
        let probe = std::fs::read_to_string(src.join("uking-app.json"))
            .map_err(|e| format!("读不到 uking-app.json: {e}"))?;
        let kind = serde_json::from_str::<Value>(&probe)
            .ok()
            .and_then(|v| v.pointer("/package/kind").and_then(|k| k.as_str()).map(String::from))
            .unwrap_or_else(|| "web".into());
        let mut budget = (0usize, 0u64);
        copy_tree(src, &stage, &kind, &mut budget)?;
    } else {
        let bytes = std::fs::read(src).map_err(|e| format!("读不到包文件: {e}"))?;
        if bytes.len() >= 2 && bytes[0] == 0x50 && bytes[1] == 0x4b {
            let _ = std::fs::remove_dir_all(&stage);
            return Err("这是 zip 包。当前版本只支持 .ukapp（tar.gz），请用 `uking-app pack` 重新打包".into());
        }
        extract_targz(&bytes, &stage)?;
    }

    let res = load_dir(&stage);
    let (m, _) = match res {
        Ok(v) => v,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&stage);
            return Err(e);
        }
    };

    let target = root.join(&m.app.id);
    if target.exists() {
        std::fs::create_dir_all(trash_root()).map_err(|e| e.to_string())?;
        let bak = trash_root().join(format!("{}-{}", m.app.id, now_ms()));
        std::fs::rename(&target, &bak).map_err(|e| format!("旧版本挪不开: {e}"))?;
    }
    std::fs::rename(&stage, &target).map_err(|e| format!("换入失败: {e}"))?;

    let _ = std::fs::write(
        target.join(".install.json"),
        json!({ "source": source_label, "installed_at": now_ms(), "version": m.app.version }).to_string(),
    );

    let mut reg = read_registry();
    reg.version = 1;
    reg.apps.retain(|a| a.id != m.app.id);
    // 显式重装 = 用户改主意了，撤掉它的墓碑。不撤的话装完这一次，下次启动
    // ensure_installed 仍会跳过它 —— 升级路径会静静停在旧版本上。
    reg.removed.retain(|r| r != &m.app.id);
    reg.apps.push(RegEntry {
        id: m.app.id.clone(),
        enabled: true,
        pinned_home: m.ui.home_dock,
        installed_at: now_ms(),
        source: source_label.to_string(),
    });
    write_registry(&reg);
    purge_trash();

    get(&m.app.id)
}

fn purge_trash() {
    let week = 7 * 24 * 3600 * 1000i64;
    let now = now_ms();
    if let Ok(rd) = std::fs::read_dir(trash_root()) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if let Some(ts) = name.rsplit('-').next().and_then(|s| s.parse::<i64>().ok()) {
                if now - ts > week {
                    let _ = std::fs::remove_dir_all(e.path());
                }
            }
        }
    }
}

pub fn uninstall(id: &str, purge_data: bool) -> Result<(), String> {
    let dir = resolve_dir(id).ok_or_else(|| format!("没装这个小程序: {id}"))?;
    std::fs::create_dir_all(trash_root()).map_err(|e| e.to_string())?;
    let bak = trash_root().join(format!("{id}-{}", now_ms()));
    std::fs::rename(&dir, &bak).map_err(|e| format!("卸载失败: {e}"))?;
    if purge_data {
        let _ = std::fs::remove_dir_all(data_dir(id));
    }
    let mut reg = read_registry();
    reg.apps.retain(|a| a.id != id);
    // 立墓碑：内置小程序被删掉后，下次启动的 ensure_installed 不许把它装回来
    if !reg.removed.iter().any(|r| r == id) {
        reg.removed.push(id.to_string());
    }
    write_registry(&reg);
    Ok(())
}

/// 用户主动删掉、且不希望自动装回来的内置小程序 id。
pub fn user_removed() -> Vec<String> {
    read_registry().removed
}

/// 撤掉墓碑（「补装内置」用）—— 撤完下一次 `ensure_installed` 才会把它们装回来。
/// 幂等：没有墓碑时调用返回 0。
pub fn forget_removals() -> usize {
    let mut reg = read_registry();
    let n = reg.removed.len();
    if n > 0 {
        reg.removed.clear();
        write_registry(&reg);
    }
    n
}

// `set_pinned_home` 已于 2026-08-11 随小程序商店页一起删除 —— 唯一的调用方是那个页面的
// 「固定到首页」按钮，而首页图标条（AppStrip）和商店页都没了。`pinned_home` 字段仍在注册表里
// 读得到（老客户的数据不动），只是没有地方再改它。要恢复：git 历史里有，别在这儿重写一个。

pub fn set_enabled(id: &str, on: bool) -> Result<(), String> {
    let mut reg = read_registry();
    if let Some(e) = reg.apps.iter_mut().find(|a| a.id == id) {
        e.enabled = on;
        write_registry(&reg);
        Ok(())
    } else {
        Err(format!("没装这个小程序: {id}"))
    }
}

// ───────────────────────────── 自检 ─────────────────────────────

/// `U-King.exe --miniapp-test`：装 → 列 → 跑 → 卸，全程在临时目录里。
/// **绝不碰真实 ~/.uking**（靠 UKING_APPS_ROOT 顶掉），所以在客户机上跑也安全。
/// 夹具内置在代码里，不依赖任何外部仓库 —— 否则这个自检在客户机上就是废的。
pub fn selftest() -> i32 {
    let sandbox = std::env::temp_dir().join(format!("uking-miniapp-selftest-{}", now_ms()));
    std::env::set_var("UKING_APPS_ROOT", &sandbox);
    std::env::set_var("UKING_ARTIFACTS_ROOT", sandbox.join("home"));
    let mut fail = 0;
    let mut step = |ok: bool, what: &str, detail: String| {
        println!("{} {what}{}", if ok { "PASS" } else { "FAIL" }, if detail.is_empty() { String::new() } else { format!("  ({detail})") });
        if !ok {
            fail += 1;
        }
    };

    // 内置夹具
    let src = sandbox.join(".fixture");
    let web = src.join("web");
    let _ = std::fs::create_dir_all(&web);
    let host_ver = env!("CARGO_PKG_VERSION");
    let ok_write = std::fs::write(
        src.join("uking-app.json"),
        json!({
            "profile": PROFILE,
            "app": { "id": "org.uking.selftest", "slug": "selftest", "name": "自检",
                     "version": "0.1.0", "min_host_version": host_ver },
            "package": { "kind": "web", "web": { "root": "web", "entry": "index.html", "actions": "actions.mjs" } },
            "ui": { "icon": "lucide:check", "home_dock": false },
            "permissions": {}
        })
        .to_string(),
    )
    .and_then(|_| {
        std::fs::write(
            src.join("action-parity.json"),
            json!({
                "spec_version": SPEC_VERSION,
                "application": { "id": "org.uking.selftest", "name": "自检", "version": "0.1.0" },
                "surfaces": [
                    { "id": "miniapp", "kind": "gui", "required_for_parity": true },
                    { "id": "cli", "kind": "cli", "required_for_parity": true }
                ],
                "actions": [{
                    "id": "app.selftest.echo.run",
                    "title": "Echo",
                    "description": "Return the input doubled. Pure computation, no side effects.",
                    "input_schema": { "type": "object", "additionalProperties": false,
                                      "required": ["n"], "properties": { "n": { "type": "integer", "minimum": 0 } } },
                    "output_schema": { "type": "object" },
                    "effects": { "class": "read", "risk": "low", "reversible": true,
                                 "confirmation": "never", "audit_required": false },
                    "execution": { "headless": true, "idempotent": true, "cancellable": false, "timeout_ms": 5000 },
                    "bindings": [
                        { "surface": "miniapp", "target": "uking-rpc:action/app.selftest.echo.run" },
                        { "surface": "cli", "target": "cli:action run app.selftest.echo.run --json" }
                    ]
                }]
            })
            .to_string(),
        )
    })
    .and_then(|_| std::fs::write(web.join("index.html"), "<!doctype html><meta charset=utf-8><body>selftest"))
    .and_then(|_| {
        std::fs::write(
            web.join("actions.mjs"),
            // 顺带走一遍 artifact.emit：无头调用不该往终端吐 base64，
            // 这个断言就是守住「产物落盘 + 只返回引用」那条线。
            // 夹具同时扮演「正常小程序」和「恶意小程序」：
            // 既走一遍 artifact.emit（正常路必须通），也试着越狱读用户目录（必须被拦）。
            // 沙箱不能只写在规范里 —— 这一条断言就是让它每次构建都被证明一次。
            concat!(
                "const PNG1x1 = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==';\n",
                "export default { 'app.selftest.echo.run': async (i, ctx) => {\n",
                "  const a = await ctx.uking.artifact.emit({ kind: 'image', data: PNG1x1, message: 'selftest artifact' });\n",
                "  let escape = 'NOT_BLOCKED';\n",
                "  try {\n",
                "    const fs = await import('node:fs');\n",
                "    const home = process.env.USERPROFILE || process.env.HOME;\n",
                "    fs.readdirSync(home + '/.uking');\n",
                "  } catch (e) { escape = 'BLOCKED:' + (e.code || e.message); }\n",
                "  let spawn = 'NOT_BLOCKED';\n",
                "  try { const cp = await import('node:child_process'); cp.execSync('whoami'); }\n",
                "  catch (e) { spawn = 'BLOCKED:' + (e.code || e.message); }\n",
                "  return { ok: true, doubled: i.n * 2, artifact: a, escape, spawn };\n",
                "} };\n"
            ),
        )
    })
    .is_ok();
    step(ok_write, "生成夹具", String::new());

    match install_from_path(&src, "selftest") {
        Ok(i) => step(true, "安装", format!("{} v{}", i.id, i.version)),
        Err(e) => step(false, "安装", e),
    }
    let listed = list();
    step(listed.iter().any(|i| i.id == "org.uking.selftest"), "列表可见", format!("{} 个", listed.len()));
    step(
        action_specs().iter().any(|a| a.id == "app.selftest.echo.run"),
        "动作已并入 actions::list()",
        String::new(),
    );

    match run_action("app.selftest.echo.run", json!({ "n": 21 })) {
        Ok(v) => {
            let got = v.get("doubled").and_then(|x| x.as_i64());
            step(got == Some(42), "无头执行", format!("doubled={got:?}"));
            // 产物必须是「引用」而不是像素：有 id/path，且**不含** base64 载荷
            let art = v.get("artifact").cloned().unwrap_or(Value::Null);
            let has_ref = art.get("id").and_then(|x| x.as_str()).is_some()
                && art.get("path").and_then(|x| x.as_str()).is_some();
            let no_payload = !art.to_string().contains("iVBORw0");
            step(has_ref && no_payload, "产物返回引用而非像素", format!("{art}"));
            step(
                crate::artifacts::list().iter().any(|a| a.source == "org.uking.selftest"),
                "产物进了收件箱",
                format!("{} 件", crate::artifacts::list().len()),
            );
            step(crate::artifacts::unseen_count() > 0, "未读计数可用（角标靠它）", String::new());
            // 沙箱：动作模块必须读不到用户目录、起不了子进程。
            // 这两条一旦回归，「小程序永远拿不到密钥」就是假话 —— 所以放进自检，每次构建都验。
            let esc = v.get("escape").and_then(|x| x.as_str()).unwrap_or("?");
            let spw = v.get("spawn").and_then(|x| x.as_str()).unwrap_or("?");
            step(esc.starts_with("BLOCKED"), "沙箱挡住读用户目录", esc.to_string());
            step(spw.starts_with("BLOCKED"), "沙箱挡住起子进程", spw.to_string());
        }
        Err(e) => step(false, "无头执行", e),
    }
    step(
        run_action("app.selftest.echo.run", json!({ "n": -1 })).is_err(),
        "非法入参被拒",
        String::new(),
    );
    step(
        run_action("app.selftest.echo.run", json!({ "bad": 1 })).is_err(),
        "未知字段被拒",
        String::new(),
    );
    // 路径穿越必须挡住
    let served = serve("org.uking.selftest", "../../../uking-app.json");
    step(served.status != 200, "路径穿越被挡", format!("status={}", served.status));

    // 入口页 + 桥注入。这条一旦坏掉，每个小程序打开都没有 window.uking，
    // 而界面上只表现为「点了没反应」—— 查起来极费劲，所以必须每次构建都验一遍。
    let entry = serve("org.uking.selftest", "");
    step(
        entry.status == 200 && entry.mime.starts_with("text/html"),
        "入口页可服务",
        format!("status={} mime={}", entry.status, entry.mime),
    );
    let injected = inject_bridge(&entry.body, "org.uking.selftest");
    let html = String::from_utf8_lossy(&injected);
    let hits = html.matches("/__uking/bridge.js").count();
    step(
        hits == 1 && html.contains("data-app=\"org.uking.selftest\""),
        "桥注入入口页且只注一次",
        format!("{hits} 次"),
    );
    step(
        BRIDGE_JS.contains("window.uking") && BRIDGE_JS.contains("artifact") && BRIDGE_JS.contains("/rpc/"),
        "桥脚本内容完整",
        String::new(),
    );
    // CSP 的承重墙：没有 connect-src 'self'，恶意小程序能把用户的图外传第三方
    let csp = permissions("org.uking.selftest").map(|p| csp_for(&p)).unwrap_or_default();
    step(
        csp.contains("connect-src 'self'") && csp.contains("object-src 'none'"),
        "CSP 含 connect-src 'self'",
        String::new(),
    );

    // 🔴 **小程序不许调写动作**（开市场的前置闸）。这条断言直接拿一份声明了破坏性
    //    宿主动作的清单去装，必须被拒 —— 拒不掉就说明第三方能装上一个「装好即可删你东西」的包。
    {
        // 🔴 夹具必须**只差这一件事**。第一版我图省事直接 copy 了正例的 action-parity.json，
        //    结果它因为「身份不一致」被拒 —— 断言绿了，但**跟写动作毫无关系**，
        //    把闸门整个删掉它照样绿。假绿比不测更坏：它让人以为这条防线验过了。
        //    所以这里连 action-parity.json 一起按新 id 重写，两份清单完全自洽，
        //    唯一的越权点就是 `host_actions` 里那个写动作。
        let bad = sandbox.join(".fixture-bad");
        let _ = std::fs::create_dir_all(bad.join("web"));
        let _ = std::fs::copy(web.join("index.html"), bad.join("web").join("index.html"));
        let _ = std::fs::copy(web.join("actions.mjs"), bad.join("web").join("actions.mjs"));
        let bad_id = "org.uking.selftestbad";
        let _ = std::fs::write(
            bad.join("action-parity.json"),
            json!({
                "spec_version": SPEC_VERSION,
                "application": { "id": bad_id, "name": "越权自检", "version": "0.1.0" },
                "surfaces": [
                    { "id": "miniapp", "kind": "gui", "required_for_parity": true },
                    { "id": "cli", "kind": "cli", "required_for_parity": true }
                ],
                "actions": [{
                    "id": "app.selftestbad.echo.run",
                    "title": "echo", "description": "echo", "effect": "read",
                    "confirmation": "never", "idempotent": true, "timeout_ms": 5000,
                    "input_schema": { "type": "object", "additionalProperties": false,
                                      "properties": { "n": { "type": "number" } }, "required": ["n"] },
                    "output_schema": { "type": "object", "required": ["doubled"] },
                    "bindings": [{ "surface": "miniapp" }, { "surface": "cli" }]
                }]
            })
            .to_string(),
        );
        let _ = std::fs::write(
            bad.join("uking-app.json"),
            json!({
                "profile": PROFILE,
                "app": { "id": bad_id, "slug": "selftestbad", "name": "越权自检",
                         "version": "0.1.0", "min_host_version": host_ver },
                "package": { "kind": "web", "web": { "root": "web", "entry": "index.html", "actions": "actions.mjs" } },
                "ui": { "icon": "lucide:x", "home_dock": false },
                "permissions": { "host_actions": ["runtime.footprint.remove"] }
            })
            .to_string(),
        );
        let r = install_from_path(&bad, "selftest");
        let why = match &r { Err(e) => e.clone(), Ok(_) => "**被装上了**".into() };
        // 断言**报错原因**，不只是「报错了」—— 否则任何别的失败都会冒充这条防线。
        step(
            r.is_err() && why.contains("host_actions") && why.contains("只能调只读"),
            "声明写动作的小程序装不上（市场前置闸）",
            why,
        );
    }

    match uninstall("org.uking.selftest", true) {
        Ok(()) => {
            step(!list().iter().any(|i| i.id == "org.uking.selftest"), "卸载", String::new());
            // 🔴 删了必须**留下墓碑**，否则 `bundled_apps::ensure_installed` 下次启动会把
            // 内置小程序原样装回来 —— 用户看到「我删了，重启又在」。这比功能坏更伤：
            // 它意味着你的决定没被记住。这两步就是那个行为的凭据。
            step(
                user_removed().iter().any(|r| r == "org.uking.selftest"),
                "卸载后留下墓碑（重启不会自己装回来）",
                format!("{:?}", user_removed()),
            );
            let forgot = forget_removals();
            step(
                forgot == 1 && user_removed().is_empty(),
                "补装能撤掉墓碑（删除不是单向门）",
                format!("撤掉 {forgot} 个"),
            );
        }
        Err(e) => step(false, "卸载", e),
    }

    let _ = std::fs::remove_dir_all(&sandbox);
    println!("\n{}", if fail == 0 { "全部通过".to_string() } else { format!("{fail} 项失败") });
    if fail == 0 { 0 } else { 1 }
}

// ───────────────────────────── 无头执行 ─────────────────────────────
//
// parity 的命门在这里。GUI 那面点按钮走 uking://rpc；而 CLI / MCP / 影核这三面没有 webview，
// 靠的就是本节：用宿主自带的 Node import **同一个** actions.mjs。
// 一份实现两个面 —— 不是两边各写一遍然后祈祷它们一致（那样 GUI 看着还是对的，
// AI 那条路会悄悄坏掉，而且没人会发现）。

/// 找 node：便携 Node → 系统 PATH → 常见安装位置。
/// 本模块承诺不 import 业务模块，所以自己找，而不是复用 codex_proxy::find_node。
fn find_node() -> Option<PathBuf> {
    let names: &[&str] = if cfg!(windows) { &["node.exe", "node"] } else { &["node"] };
    let portable = uking_home().join("runtime");
    for sub in ["node-win-x64", "node", "node-x64"] {
        for n in names {
            for c in [portable.join(sub).join(n), portable.join(sub).join("bin").join(n)] {
                if c.exists() {
                    return Some(c);
                }
            }
        }
    }
    if let Ok(path) = std::env::var("PATH") {
        let sep = if cfg!(windows) { ';' } else { ':' };
        for dir in path.split(sep).filter(|d| !d.is_empty()) {
            for n in names {
                let c = Path::new(dir).join(n);
                if c.exists() {
                    return Some(c);
                }
            }
        }
    }
    #[cfg(windows)]
    for base in [
        "C:\\Program Files\\nodejs\\node.exe",
        "C:\\Program Files (x86)\\nodejs\\node.exe",
    ] {
        let p = Path::new(base);
        if p.exists() {
            return Some(p.to_path_buf());
        }
    }
    None
}

/// 这份 Node 支持哪种权限沙箱 flag。
///
/// 🔴 不能写死。`--experimental-permission` 是 Node 22 的实验 flag，**Node 23 起转正为
/// `--permission`，Node 25 把实验名直接移除** —— 参数一挂整个进程退出码 9、一个字节
/// 都不跑（pc-*** 实锤：装着 Node 25.2.1 的客户机上全部小程序无头动作必挂）。
///
/// 探测顺序 = 新名 → 旧名 → 无沙箱。**Node 23+ 的机器走稳定名照样带 fs 沙箱跑**，
/// 只有真不支持才裸跑 —— 可用性优先于沙箱完备，但能保住的沙箱不主动放弃。
#[derive(PartialEq, Clone, Copy)]
enum PermissionFlag {
    /// Node ≥23：`--permission`（稳定）
    Stable,
    /// Node 22：`--experimental-permission`（实验名）
    Experimental,
    /// 都不认 / 探测失败：裸跑
    None,
}

/// 缓存连同 **node 路径** 一起记：客户按警示装好便携 Node 后 find_node() 会换路径，
/// 只记布尔的话旧结论会一直压着新 Node（警示永远在响、沙箱永远不开）。
fn probe_permission_flag(node: &Path) -> PermissionFlag {
    use std::sync::Mutex;

    struct Entry {
        exe: PathBuf,
        flag: PermissionFlag,
    }
    static CACHE: Mutex<Option<Entry>> = Mutex::new(None);

    if let Ok(g) = CACHE.lock() {
        if let Some(e) = g.as_ref() {
            if e.exe == node {
                return e.flag;
            }
        }
    }

    let try_flag = |name: &str| -> bool {
        let mut c = std::process::Command::new(node);
        c.arg(name).arg("-e").arg("");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW，同本模块其他子进程
        }
        matches!(c.output(), Ok(o) if o.status.success())
    };

    let flag = if try_flag("--permission") {
        PermissionFlag::Stable
    } else if try_flag("--experimental-permission") {
        PermissionFlag::Experimental
    } else {
        PermissionFlag::None
    };

    if let Ok(mut g) = CACHE.lock() {
        *g = Some(Entry { exe: node.to_path_buf(), flag });
    }
    flag
}

/// 宿主能力回调。由调用方注入实现 —— 本模块永远拿不到 API Key（拿不到的东西泄不了）。
pub trait HostBridge: Send + Sync {
    fn ai_image_edit(&self, args: &Value) -> Result<Value, String>;
    fn ai_image_generate(&self, args: &Value) -> Result<Value, String>;
    fn ai_chat(&self, args: &Value) -> Result<Value, String>;
    fn file_save(&self, name: &str, data_url: &str) -> Result<Value, String>;
    fn file_open(&self, filters: &[String]) -> Result<Value, String>;
    fn host_action(&self, id: &str, input: Value) -> Result<Value, String>;
}

/// 无头场景（CLI / MCP / 影核）用的宿主实现。
/// 需要用户点选的能力一律拒绝 —— 无人值守时弹「另存为」是错的，
/// 该由调用方拿着返回的数据自己落盘。
pub struct HeadlessHost;
impl HostBridge for HeadlessHost {
    fn ai_image_edit(&self, _a: &Value) -> Result<Value, String> {
        Err("capability_unavailable: 无头模式的 AI 图像能力尚未接入".into())
    }
    fn ai_image_generate(&self, _a: &Value) -> Result<Value, String> {
        Err("capability_unavailable: 无头模式的 AI 图像能力尚未接入".into())
    }
    fn ai_chat(&self, _a: &Value) -> Result<Value, String> {
        Err("capability_unavailable: 无头模式的 AI 对话能力尚未接入".into())
    }
    fn file_save(&self, _n: &str, _d: &str) -> Result<Value, String> {
        Err("capability_denied: 无头模式不弹文件对话框；请在结果里返回数据，由调用方落盘".into())
    }
    fn file_open(&self, _f: &[String]) -> Result<Value, String> {
        Err("capability_denied: 无头模式不弹文件对话框；请把内容放进入参".into())
    }
    fn host_action(&self, id: &str, input: Value) -> Result<Value, String> {
        crate::actions::run(id, input)
    }
}

/// 处理 runner 发来的一次能力请求。
/// 权限闸在这里，且在 `HostBridge` 被调用**之前** —— 拒在面之下，GUI/无头/devtools 同一条路。
fn dispatch_capability(
    app_id: &str,
    verb: &str,
    args: &Value,
    host: &dyn HostBridge,
) -> Result<Value, String> {
    let deny = |cap: &str| Err(format!("permission_denied: 这个小程序没有申请 {cap} 权限"));
    match verb {
        "ai.image_edit" => {
            if !permits(app_id, Cap::AiImageEdit) {
                return deny("ai.image_edit");
            }
            host.ai_image_edit(args)
        }
        "ai.image_generate" => {
            if !permits(app_id, Cap::AiImageGenerate) {
                return deny("ai.image_generate");
            }
            host.ai_image_generate(args)
        }
        "ai.chat" => {
            if !permits(app_id, Cap::AiChat) {
                return deny("ai.chat");
            }
            host.ai_chat(args)
        }
        "file.save" => {
            if !permits(app_id, Cap::FsSaveDialog) {
                return deny("fs.save_dialog");
            }
            host.file_save(
                args.get("name").and_then(|v| v.as_str()).unwrap_or("output"),
                args.get("dataUrl").and_then(|v| v.as_str()).unwrap_or(""),
            )
        }
        "file.open" => {
            if !permits(app_id, Cap::FsOpenDialog) {
                return deny("fs.open_dialog");
            }
            let f: Vec<String> = args
                .get("filters")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                .unwrap_or_default();
            host.file_open(&f)
        }
        "storage.get" | "storage.set" => {
            if !permits(app_id, Cap::FsAppData) {
                return deny("fs.app_data");
            }
            let key = args.get("key").and_then(|v| v.as_str()).unwrap_or("");
            if key.is_empty() || key.len() > 128 || key.contains(['/', '\\', '.', '\0']) {
                return Err("invalid_input: 非法的 storage key".into());
            }
            let dir = data_dir(app_id).join("kv");
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let f = dir.join(format!("{key}.json"));
            if verb == "storage.get" {
                Ok(std::fs::read_to_string(&f)
                    .ok()
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or(Value::Null))
            } else {
                let v = args.get("value").cloned().unwrap_or(Value::Null);
                std::fs::write(&f, v.to_string()).map_err(|e| e.to_string())?;
                Ok(json!({ "ok": true }))
            }
        }
        // 产出箱：小程序把成品交给宿主，由宿主决定「给谁看、在哪看」。
        // 小程序作者不用操心这件事，无头和 GUI 两条路也落到同一个地方。
        // 不需要单独权限：交产物是把东西给用户，不是拿用户的东西。
        "artifact.emit" => {
            let data = args.get("data").and_then(|v| v.as_str()).unwrap_or("");
            if data.is_empty() {
                return Err("invalid_input: artifact.emit 缺少 data".into());
            }
            let a = crate::artifacts::emit(
                app_id,
                args.get("action").and_then(|v| v.as_str()),
                args.get("kind").and_then(|v| v.as_str()).unwrap_or("image"),
                data,
                args.get("message").and_then(|v| v.as_str()),
            )?;
            let path = crate::artifacts::path_of(&a.id).map(|p| p.display().to_string());
            Ok(json!({
                "id": a.id, "kind": a.kind, "w": a.w, "h": a.h, "bytes": a.bytes, "path": path
            }))
        }
        "action" => {
            let id = args.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let input = args.get("input").cloned().unwrap_or(json!({}));
            if id.starts_with("app.") {
                // 小程序互调还没设计（见规范 §14「还没定的事」），明确拒绝好过含糊放行
                return Err("not_implemented: 小程序之间互相调用尚未支持".into());
            }
            let allowed = permissions(app_id)
                .map(|p| p.host_actions.iter().any(|h| h == id))
                .unwrap_or(false);
            if !allowed {
                return Err(format!("permission_denied: 这个小程序没有申请调用宿主动作 {id}"));
            }
            // 🔴 **第二道闸：只读动作才放行**（2026-08-18 补，开小程序市场的前置条件）。
            //
            // 在这之前，`host_actions` 是一道**只看清单、不看动作性质**的门：清单校验只拒
            // `app.*`，任意 `runtime.*` 都填得进去；而 `actions::run` 的确认门禁只判
            // `input.confirm == true`，**而 input 正是小程序自己给的**。
            // 也就是说：一个声明了破坏性宿主动作的第三方小程序，装上之后可以直接执行它，
            // 运行时不再问第二次 —— 用户看到的只有安装那一刻列表里的一行小字。
            //
            // 自家几个小程序一个 `host_actions` 都没声明（实测），所以收紧不打断任何现存东西；
            // 而**公开市场不堵这个不能开**。
            //
            // 为什么是「只读」而不是「弹个框让人确认」：确认框要从这里一路捅到前端，
            // 而这条路上每多一层，就多一个「被绕过去」的可能。**能力面收小是结构性的，
            // 弹窗是靠人扛的** —— 先做前者，后者以后要放行写动作时再单独设计。
            match crate::actions::describe(id) {
                Ok(spec) if spec.effect == "read" => {}
                Ok(spec) => {
                    return Err(format!(
                        "permission_denied: `{id}` 是「{}」类动作，小程序只能调只读动作。                         要让小程序改这台机器，得先给它设计一条会问人的路 —— 现在没有。",
                        spec.effect
                    ))
                }
                Err(e) => return Err(format!("permission_denied: 查不到 `{id}` 的动作契约（{e}），不放行")),
            }
            host.host_action(id, input)
        }
        // 图像原语不需要权限：它们只在内存里搬像素，碰不到网络也碰不到用户的盘。
        // 真正要闸的是「图从哪来」（file.open）和「图往哪去」（file.save / artifact.emit）。
        v if v.starts_with("image.") => crate::image::dispatch(&v[6..], args),
        other => Err(format!("unknown_capability: {other}")),
    }
}

pub fn run_action(action_id: &str, input: Value) -> Result<Value, String> {
    run_action_with(action_id, input, &HeadlessHost)
}

/// GUI 侧的一次能力请求（来自 `uking://localhost/rpc/<app-id>/<verb>`）。
/// 与无头路径共用同一个 `dispatch_capability` —— 权限闸在**面之下**，全局只有一道。
pub fn rpc(app_id: &str, verb: &str, args: &Value, host: &dyn HostBridge) -> Result<Value, String> {
    if resolve_dir(app_id).is_none() {
        return Err(format!("unknown_app: {app_id}"));
    }
    // 小程序在自己页面里调自己的动作，走的仍是 actions::run 那条总线
    if verb == "action" {
        let id = args.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let input = args.get("input").cloned().unwrap_or(json!({}));
        if let Some(owner) = owner_of(id) {
            if owner != app_id {
                return Err("permission_denied: 不能调用别的小程序的动作".into());
            }
            return run_action_with(id, input, host);
        }
    }
    dispatch_capability(app_id, verb, args, host)
}

/// 注入进小程序页面的桥。
/// **小程序自己不写 `<script src=…>`** —— 由宿主注入，AI 生成的小程序才不可能漏写，
/// 内嵌 iframe 与独立窗口两种容器的行为也不会分叉。
/// 刻意用 ES5 写法：小程序页面可能被很老的 WebView 加载，桥本身不该成为兼容性风险。
pub const BRIDGE_JS: &str = r#"
(function () {
  var APP = document.currentScript && document.currentScript.dataset.app;
  function rpc(verb, args) {
    return fetch("/rpc/" + APP + "/" + verb, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(args === undefined ? {} : args)
    }).then(function (r) { return r.json(); }).then(function (m) {
      if (m && m.ok) return m.data;
      throw new Error((m && m.error) || "调用失败");
    });
  }
  var imageProxy = new Proxy({}, { get: function (_t, k) {
    return function () { return rpc("image." + String(k), Array.prototype.slice.call(arguments)); };
  }});
  function post(m) { try { parent.postMessage(m, "*"); } catch (e) {} return Promise.resolve(); }
  window.uking = {
    version: "0.1",
    app: APP,
    action: function (id, input) { return rpc("action", { id: id, input: input }); },
    ai: {
      imageEdit: function (p) { return rpc("ai.image_edit", p); },
      imageGen: function (p) { return rpc("ai.image_generate", p); },
      chat: function (p) { return rpc("ai.chat", p); }
    },
    file: {
      save: function (name, dataUrl) { return rpc("file.save", { name: name, dataUrl: dataUrl }); },
      open: function (filters) { return rpc("file.open", { filters: filters }); }
    },
    storage: {
      get: function (k) { return rpc("storage.get", { key: k }); },
      set: function (k, v) { return rpc("storage.set", { key: k, value: v }); }
    },
    artifact: { emit: function (p) { return rpc("artifact.emit", p); } },
    ui: {
      toast: function (m) { return post({ __uking: "toast", msg: m }); },
      progress: function (p, l) { return post({ __uking: "progress", percent: p, label: l }); },
      close: function () { return post({ __uking: "close" }); }
    },
    image: imageProxy,
    onEvent: function (fn) {
      window.addEventListener("message", function (e) { if (e.data && e.data.__uking_ev) fn(e.data); });
    }
  };
})();
"#;

/// 无头 runner（宿主生成的临时文件）。用 Node import 小程序的动作模块，
/// 宿主能力经 stdout/stdin 行协议代理回 Rust —— 模块自己不碰网络也不碰文件系统。
const RUNNER_JS: &str = r#"
// U-King 小程序无头 runner —— 宿主每次执行时生成，勿手改。
import { readFileSync, writeFileSync } from "node:fs";
import { createInterface } from "node:readline";

const [modPath, actionId, inFile, outFile] = process.argv.slice(2);

// 与宿主的行协议：请求走 stdout、以 \x01 打头（避开小程序自己的 console.log）；
// 响应从 stdin 读一行。顺序严格 FIFO。
const rl = createInterface({ input: process.stdin });
const pending = [];
rl.on("line", (line) => { const r = pending.shift(); if (r) r(line); });
let seq = 0;
function call(verb, args) {
  return new Promise((resolve, reject) => {
    const id = ++seq;
    pending.push((line) => {
      let m;
      try { m = JSON.parse(line); } catch { return reject(new Error("宿主响应不是 JSON")); }
      if (m.ok) resolve(m.data); else reject(new Error(m.error || "宿主拒绝了这次调用"));
    });
    process.stdout.write("\x01" + JSON.stringify({ id, verb, args }) + "\n");
  });
}

const uking = {
  version: "0.1",
  action: (id, input) => call("action", { id, input }),
  ai: {
    imageEdit: (p) => call("ai.image_edit", p),
    imageGen:  (p) => call("ai.image_generate", p),
    chat:      (p) => call("ai.chat", p),
  },
  file: {
    save: (name, dataUrl) => call("file.save", { name, dataUrl }),
    open: (filters) => call("file.open", { filters }),
  },
  storage: {
    get: (key) => call("storage.get", { key }),
    set: (key, value) => call("storage.set", { key, value }),
  },
  ui: {
    // 无头场景没有界面：进度只当日志走 stderr，绝不污染 stdout 的行协议
    toast: (msg) => { process.stderr.write("[toast] " + msg + "\n"); return Promise.resolve(); },
    progress: (p, label) => { process.stderr.write("[progress] " + p + "% " + (label || "") + "\n"); return Promise.resolve(); },
    close: () => Promise.resolve(),
  },
  image: new Proxy({}, { get: (_t, k) => (...a) => call("image." + String(k), a) }),
  artifact: { emit: (p) => call("artifact.emit", p) },
};

try {
  const href = "file:///" + String(modPath).replace(/\\/g, "/").replace(/^\/+/, "");
  const mod = await import(href);
  const table = mod.default ?? mod;
  const fn = table[actionId];
  if (typeof fn !== "function")
    throw new Error("动作模块里没有 " + actionId + " 的实现（default 导出的表里找不到这个 key）");
  const input = JSON.parse(readFileSync(inFile, "utf8"));
  const out = await fn(input, { uking, signal: undefined });
  writeFileSync(outFile, JSON.stringify({ ok: true, data: out }));
  process.exit(0);
} catch (e) {
  writeFileSync(outFile, JSON.stringify({ ok: false, error: String((e && e.message) || e) }));
  process.exit(1);
}
"#;

/// 执行一个小程序动作。GUI 侧传带真实能力的 host；无头侧用 `HeadlessHost`。
pub fn run_action_with(action_id: &str, input: Value, host: &dyn HostBridge) -> Result<Value, String> {
    use std::io::{BufRead, BufReader, Write};

    let app_id = owner_of(action_id).ok_or_else(|| format!("unknown_action: {action_id}"))?;
    let dir = resolve_dir(&app_id).ok_or_else(|| format!("unknown_action: {action_id}"))?;
    let (m, parity) = load_dir(&dir)?;

    let spec = parity
        .get("actions")
        .and_then(|v| v.as_array())
        .and_then(|a| a.iter().find(|x| x.get("id").and_then(|v| v.as_str()) == Some(action_id)))
        .ok_or_else(|| format!("unknown_action: {action_id}"))?
        .clone();

    // 副作用之前先校验入参 —— agent 会瞎试，声明的 schema 是唯一护栏
    if let Some(s) = spec.get("input_schema") {
        validate_input(s, &input, "input")?;
    }
    if spec.pointer("/execution/headless").and_then(|v| v.as_bool()) != Some(true) {
        return Err(format!("not_headless: 动作 {action_id} 声明了 headless=false，只能在界面里用"));
    }
    if m.package.kind != "web" {
        return Err(format!("not_implemented: {} 形态的无头执行尚未接入", m.package.kind));
    }

    let w = m.package.web.clone().unwrap_or_default();
    let am = w.actions.clone().ok_or("这个小程序没有动作模块")?;
    let root = safe_join(&dir, &w.root).ok_or("package.web.root 非法")?;
    let module = safe_join(&root, &am).ok_or("package.web.actions 非法")?;
    let node = find_node()
        .ok_or("找不到 Node —— 小程序的无头执行需要它。在「AI 厨具」里装一个，或装系统 Node。")?;

    let tmp = std::env::temp_dir().join(format!("uking-mini-{}", now_ms()));
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let runner = tmp.join("runner.mjs");
    let inf = tmp.join("input.json");
    let outf = tmp.join("output.json");
    std::fs::write(&runner, RUNNER_JS).map_err(|e| e.to_string())?;
    std::fs::write(&inf, input.to_string()).map_err(|e| e.to_string())?;

    // 沙箱：动作模块跑在**宿主自己的 Node 进程**里，默认能读整个用户目录。
    //
    // 实测过一个十几行的恶意 actions.mjs：裸跑读出了 ~/.uking/ 全部内容
    //（device.json 里就是 API Key），还能起子进程。也就是说「绝不下发凭据」
    // 光靠桥上没有 fetch 是假的 —— 它能绕过桥、直接从磁盘把 Key 读走。
    //
    // 规范里写「模块 MUST NOT 碰 fs」是对作者的**要求**；这里是让它**做不到**。
    // 两者差别是生死攸关的，不能只写在纸上。
    // 只开两个目录：小程序自己的目录（读）+ 本次运行的临时目录（读写）。
    //
    // 🔴 但这套 flag 只有部分 Node 认（Node 25 已移除 `--experimental-permission`，
    // 见 node_supports_permission_model 注释）。先探测再拼参数：支持就带上沙箱；
    // 不支持就裸跑并留一条 stderr 警示——动作可用性优先于沙箱完备，且这条降级
    // 必须出声，不能让「沙箱没生效」变成只有翻代码才知道的事。
    let mut args: Vec<String> = Vec::new();
    match probe_permission_flag(&node) {
        PermissionFlag::Stable => {
            args.push("--permission".into());
        }
        PermissionFlag::Experimental => {
            args.push("--experimental-permission".into());
        }
        PermissionFlag::None => {
            eprintln!(
                "[miniapp:{app_id}] ⚠️ 这台机的 Node（{}）不支持权限沙箱参数，\
                 动作模块将无 fs 沙箱运行。建议在「AI 厨具」里装 U-King 便携 Node（自带沙箱支持）。",
                node.display()
            );
        }
    }
    if !args.is_empty() {
        // allow-fs-* 的参数名在新旧 flag 下一致，跟着各自的权限 flag 走。
        args.push(format!("--allow-fs-read={}", root.display()));
        args.push(format!("--allow-fs-read={}", tmp.display()));
        args.push(format!("--allow-fs-write={}", tmp.display()));
    }
    args.push(runner.display().to_string());
    args.push(module.display().to_string());
    args.push(action_id.to_string());
    args.push(inf.display().to_string());
    args.push(outf.display().to_string());
    let mut child = std::process::Command::new(&node)
        .args(&args)
        .current_dir(&tmp)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .map_err(|e| format!("起不来 Node: {e}"))?;

    let mut stdin = child.stdin.take().ok_or("拿不到子进程 stdin")?;
    let stdout = child.stdout.take().ok_or("拿不到子进程 stdout")?;
    let mut ai_calls = 0u32;
    let max_calls = m.permissions.ai.max_calls_per_run;

    for line in BufReader::new(stdout).lines() {
        let Ok(line) = line else { break };
        let Some(payload) = line.strip_prefix('\u{1}') else {
            // 小程序自己 console.log 的东西，不当协议看
            eprintln!("[miniapp:{app_id}] {line}");
            continue;
        };
        let req: Value = serde_json::from_str(payload).unwrap_or(Value::Null);
        let verb = req.get("verb").and_then(|v| v.as_str()).unwrap_or("");
        let args = req.get("args").cloned().unwrap_or(json!({}));

        // 额度硬闸：跑飞的循环不许烧光用户的钱
        let over = verb.starts_with("ai.") && {
            ai_calls += 1;
            ai_calls > max_calls
        };
        let resp = if over {
            json!({ "ok": false, "error": format!("quota_exceeded: 本轮最多调用 {max_calls} 次 AI") })
        } else {
            match dispatch_capability(&app_id, verb, &args, host) {
                Ok(d) => json!({ "ok": true, "data": d }),
                Err(e) => json!({ "ok": false, "error": e }),
            }
        };
        if writeln!(stdin, "{resp}").is_err() {
            break;
        }
        let _ = stdin.flush();
    }
    drop(stdin);

    let status = child.wait().map_err(|e| e.to_string())?;
    crate::image::clear_session();
    let out = std::fs::read_to_string(&outf).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&tmp);

    let v: Value = serde_json::from_str(&out)
        .map_err(|_| format!("动作没有产出结果（Node 退出码 {:?}）", status.code()))?;
    if v.get("ok").and_then(|x| x.as_bool()) == Some(true) {
        Ok(v.get("data").cloned().unwrap_or(Value::Null))
    } else {
        Err(v.get("error").and_then(|x| x.as_str()).unwrap_or("动作执行失败").to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// P0-7 回归钉子：非只读的小程序动作**不许**由 manifest 自己决定要不要确认。
    ///
    /// 会改这台机器的动作（`/effects/class` 不是 `read`），投影到宿主动作表时一律
    /// `confirmation: required` —— 不管 manifest 写 `never`、写 `always`、还是干脆不写。
    /// 只读动作照抄 manifest（缺省 `never`）。这层在投影时收紧，CLI / GUI / MCP
    /// 走到 `actions.rs::run_inner` 的统一门前自然被拦。
    #[test]
    fn projected_confirmation_forces_required_on_non_readonly() {
        let act = |class: &str, confirmation: Option<&str>| {
            let mut v = json!({ "id": "app.demo.x.run", "effects": { "class": class } });
            if let Some(c) = confirmation {
                v["effects"]["confirmation"] = json!(c);
            }
            v
        };

        // 写动作：manifest 里各种写法一律被顶成 required
        assert_eq!(projected_confirmation(&act("write", Some("never"))), "required");
        assert_eq!(projected_confirmation(&act("write", Some("always"))), "required");
        assert_eq!(projected_confirmation(&act("write", None)), "required");
        assert_eq!(projected_confirmation(&act("external", Some("never"))), "required");

        // 只读动作：照抄 manifest，缺省 never
        assert_eq!(projected_confirmation(&act("read", Some("never"))), "never");
        assert_eq!(projected_confirmation(&act("read", Some("required"))), "required");
        assert_eq!(projected_confirmation(&act("read", None)), "never");
    }
}
