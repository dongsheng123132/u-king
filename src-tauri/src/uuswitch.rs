//! uu-switch —— 去广告版 cc-switch AI 模型切换器（我方 fork）安装 + 启动。
//!
//! **可插拔模块**（遵本项目「模块独立」铁律）：只暴露纯函数，`#[tauri::command]`
//! 一律写在 `lib.rs` 转调；进度用 `|msg|` 回调传出，`lib.rs` 再 `emit`。删掉本模块
//! 只需动：`lib.rs`（去 `mod uuswitch` + 2 个 command + generate_handler 两行）、
//! `tools.rs`（去 1 张卡片 + launch_app 里 1 条路由）、`App.tsx`（去 1 个 openTool 分支）。
//! 依赖方向只向老的公共助手（`installer::system_tool`）借力，不反向牵动别处。
//!
//! ## 分发
//! 安装包托管在我方下载源（同 U-King 下载约定，固定名 `uu-switch-Setup.msi`，不带版本号）：
//!   - 国内可达主源 `u-claw.org.cn/download/`（自有服务器，SNI 不被墙）
//!   - OSS 兜底直链 `u-claw-updates.oss-cn-shenzhen.aliyuncs.com/uking/`
//! 未上 GitHub（用户 2026-07-23 决定：暂不 push），故不走 GitHub releases。
//!
//! ## Windows 落点（实测 tauri WiX per-user MSI）
//! 安装到 `%LOCALAPPDATA%\Programs\uu-switch\`；内部二进制沿用上游 crate 名 `cc-switch.exe`
//! （productName=uu-switch 只改**显示名/快捷方式/窗口标题**，未改 mainBinaryName，故 exe 仍叫
//! cc-switch.exe）。检测「已装」= 探测该 exe。静默安装 = `msiexec /i <msi> /qn`（per-user 免管理员）。
//! ⚠️ 上游 tauri.conf 只出了 WiX MSI（NSIS 助手 DLL 需联网下载、开发机代理下载失败），故用 MSI。

#[allow(unused_imports)]
use std::path::{Path, PathBuf};
use serde_json::{json, Value};

/// 国内可达主源（自有服务器下载目录）。
const UUSWITCH_URL_CN: &str = "https://u-claw.org.cn/download/uu-switch-Setup.msi";
/// OSS 兜底直链（深圳）。主源不畅时自动切这条。
const UUSWITCH_URL_OSS: &str =
    "https://u-claw-updates.oss-cn-shenzhen.aliyuncs.com/uking/uu-switch-Setup.msi";

/// 前端「打开下载页 / 手动装」兜底用的下载直链（返回国内主源）。
pub fn download_url() -> String {
    UUSWITCH_URL_CN.to_string()
}

/// 在 tauri WiX per-user MSI 落点找 uu-switch 的 exe（检测「已装」与「启动」共用）。
#[cfg(windows)]
pub fn find_exe() -> Option<PathBuf> {
    let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
    let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".into());
    let roots = [Path::new(&local).join("Programs"), PathBuf::from(&pf)];
    // 二进制名是稳定的（cc-switch.exe，见文件头说明）；**目录名不是**。
    const EXES: [&str; 2] = ["cc-switch.exe", "uu-switch.exe"];

    // ① 快路径：已知目录名直接命中，不扫盘。
    //    实测上游 MSI 装出来的是「CC Switch」（带空格、首字母大写），不是「uu-switch」——
    //    原来只找 uu-switch\ 的写法在装过的机器上恒判「未安装」，于是每次点安装都重下重装，
    //    MSI 发现同一产品已在就返回 1603，再回退可视安装 → 客户看到「无法打开此安装程序包」，
    //    而且这个循环永远出不来（线上实测 2026-07-27）。
    for root in &roots {
        for dir in ["uu-switch", "CC Switch", "cc-switch"] {
            for exe in EXES {
                let p = root.join(dir).join(exe);
                if p.exists() {
                    return Some(p);
                }
            }
        }
    }

    // ② 兜底：目录名再变也不怕 —— 浅扫这两个根目录，谁下面有那个 exe 就是它。
    //    只看一层子目录，成本可忽略；绝不递归全盘。
    for root in &roots {
        let Ok(rd) = std::fs::read_dir(root) else { continue };
        for e in rd.flatten() {
            if !e.path().is_dir() {
                continue;
            }
            for exe in EXES {
                let p = e.path().join(exe);
                if p.exists() {
                    return Some(p);
                }
            }
        }
    }
    None
}

/// 校验下载的文件确实是**完整的** MSI（OLE2 复合文档），再决定要不要交给 msiexec。
/// 两道关，缺一不可：
///  ① 头 8 字节 = OLE2 魔数 `D0 CF 11 E0 A1 B1 1A E1`——挡住 HTML 404 页 / nginx 报错页 / 垃圾内容；
///  ② 大小 ≥ `MIN_MSI_SIZE`——挡住「curl 退出 0 但被代理/CDN/杀软截短」的半截包（半截 MSI 头部魔数
///     仍在，只有大小能识破）。真包实测 ~12.4 MB，阈值取 10 MB：既能识破半截，又给版本体积留足余量。
///
/// **为什么必须先校验**：坏包一旦喂给 `msiexec`，它会弹「无法打开此安装程序包」(MSI 1620) 的吓人
/// 系统框——客户根本不知道是网络问题。从根上不喂坏包，就永远不弹那个框（改成 U-King 自己的人话提示）。
#[cfg(windows)]
fn looks_like_msi(path: &Path) -> bool {
    use std::io::Read;
    const OLE2_MAGIC: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
    const MIN_MSI_SIZE: u64 = 10_000_000;
    match std::fs::metadata(path) {
        Ok(m) if m.len() >= MIN_MSI_SIZE => {}
        _ => return false,
    }
    let mut head = [0u8; 8];
    match std::fs::File::open(path) {
        Ok(mut f) => f.read_exact(&mut head).is_ok() && head == OLE2_MAGIC,
        Err(_) => false,
    }
}

/// uu-switch 是否已装。仅 Windows 有安装/启动能力；Mac 探 `/Applications`（当前未发 Mac 包）。
pub fn installed() -> bool {
    #[cfg(windows)]
    {
        find_exe().is_some()
    }
    #[cfg(not(windows))]
    {
        Path::new("/Applications/uu-switch.app").exists()
    }
}

/// uu-switch 的 provider 配置文件 `~/.cc-switch/config.json`。品牌虽是 uu-switch，配置目录
/// 沿用上游 `.cc-switch`（「功能性字符串故意不改」，见文件头 + SYNC.md）。v2 结构：
/// `{ "version":2, "claude":{providers:{},current}, "codex":{...}, ... }`。
fn cc_switch_config_path() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    Path::new(&home).join(".cc-switch").join("config.json")
}

// 我方导入项的固定 id（幂等：重复导入 = 更新同一条，不堆重复）。
const XIAPAN_CLAUDE_ID: &str = "uking-xiapan";
const XIAPAN_CODEX_ID: &str = "uking-xiapan-codex";
const CURRENT_CLAUDE_ID: &str = "uking-current-claude";
const CURRENT_CODEX_ID: &str = "uking-current-codex";

// 虾盘云端点（与 U-King providers.rs 单一真相源一致：用国内可达的 .cn 域，避开 GFW SNI）。
const XP_ANTHROPIC_BASE: &str = "https://api.u-claw.org.cn";
const XP_OPENAI_BASE: &str = "https://api.u-claw.org.cn/v1";

fn home_dir() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    PathBuf::from(home)
}

/// 虾盘云 Claude provider（settingsConfig 对齐 cc-switch env 6 键格式，uu-switch 界面才认得）。
fn xiapan_claude_provider(key: &str) -> Value {
    json!({
        "id": XIAPAN_CLAUDE_ID,
        "name": "虾盘云·Claude（U-King）",
        "settingsConfig": { "env": {
            "ANTHROPIC_BASE_URL": XP_ANTHROPIC_BASE,
            "ANTHROPIC_AUTH_TOKEN": key,
            "ANTHROPIC_MODEL": "deepseek-v4-pro",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": "deepseek-v4-flash",
            "ANTHROPIC_DEFAULT_SONNET_MODEL": "deepseek-v4-pro",
            "ANTHROPIC_DEFAULT_OPUS_MODEL": "deepseek-v4-pro",
        }},
        "websiteUrl": "https://u-claw.org.cn/uking/",
        "category": "custom",
        "icon": "anthropic",
        "notes": "U-King 一键导入。满血 deepseek-v4-pro，设备 Key 计费。",
    })
}

/// 虾盘云 Codex provider。settingsConfig = `{auth, config}`（cc-switch 会把 config 原样写
/// `~/.codex/config.toml`、auth 写 `auth.json`）。config.toml 对齐 U-King providers.rs::apply_codex
/// 的 responses 链路：`experimental_bearer_token` + `requires_openai_auth=false` +
/// `x-openai-actor-authorization` 头（新版 Codex CLI/App 只认 responses）。
///
/// 模型**必须**取 `providers::xiapan_codex_model()`，不许在这里写死：这里写死过
/// `gpt-5.3-codex`，而 U-King 自己早就默认走便宜的 `deepseek-v4-flash-codex` ——
/// 客户从 uu-switch 那边切一下就被切回贵几十倍的模型，还不知道为什么账单涨了。
fn xiapan_codex_provider(key: &str) -> Value {
    let model = crate::providers::xiapan_codex_model();
    let config_toml = format!(
        // `disable_response_storage` 2026-08-24 一并删掉（同 `providers::apply_codex` 那处，
        // 理由见那边的长注释）：codex 0.149 把它判成 unknown field，`--strict-config` 下
        // **整份 config.toml 拒载**。这里是第二份拷贝 —— 同一事实存在几份就会漂几份（宪法 8），
        // 改一处必须搜一次全仓，别留下一个「只有 uu-switch 那条路还是坏的」。
        "model = \"{model}\"\nmodel_provider = \"xiapan\"\n\n[model_providers.xiapan]\nname = \"虾盘云\"\nbase_url = \"{base}\"\nexperimental_bearer_token = \"{key}\"\nwire_api = \"responses\"\nrequires_openai_auth = false\nhttp_headers = {{ \"x-openai-actor-authorization\" = \"u-king\" }}\n",
        model = model,
        base = XP_OPENAI_BASE,
        key = key,
    );
    json!({
        "id": XIAPAN_CODEX_ID,
        "name": "虾盘云·Codex（U-King）",
        "settingsConfig": {
            "auth": { "OPENAI_API_KEY": key },
            "config": config_toml,
        },
        "websiteUrl": "https://u-claw.org.cn/uking/",
        "category": "custom",
        "icon": "openai",
        "notes": format!("U-King 一键导入。{model}，responses 链路。"),
    })
}

/// 读用户在用的 Claude 配置（`~/.claude/settings.json`）→ provider。仅当有 base_url 且**不是
/// 虾盘云**时导（虾盘云已单独导入；官方直连=无 base_url，无需导「空配置」）。
fn current_claude_provider() -> Option<Value> {
    let p = home_dir().join(".claude").join("settings.json");
    let s = std::fs::read_to_string(&p).ok()?;
    let v: Value = serde_json::from_str(&s).ok()?;
    let base = v
        .pointer("/env/ANTHROPIC_BASE_URL")
        .and_then(|x| x.as_str())
        .unwrap_or("");
    if base.trim().is_empty() || crate::providers::is_xiapan_endpoint(&base) {
        return None;
    }
    Some(json!({
        "id": CURRENT_CLAUDE_ID,
        "name": "我在用的 Claude 配置（导入）",
        "settingsConfig": v,   // 整份 settings.json 作为 settingsConfig（cc-switch 会原样写回）
        "category": "custom",
        "icon": "anthropic",
        "notes": format!("从 ~/.claude/settings.json 导入（{base}）。"),
    }))
}

/// 读用户在用的 Codex 配置（`~/.codex/config.toml` + `auth.json`）→ provider。跳过 U-King 托管的
/// 虾盘云配置（含 `managed by U-King` 标记，已单独导入）。
fn current_codex_provider() -> Option<Value> {
    let dir = home_dir().join(".codex");
    let cfg = std::fs::read_to_string(dir.join("config.toml")).ok()?;
    if cfg.trim().is_empty() || cfg.contains("managed by U-King") {
        return None;
    }
    let auth: Value = std::fs::read_to_string(dir.join("auth.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({}));
    Some(json!({
        "id": CURRENT_CODEX_ID,
        "name": "我在用的 Codex 配置（导入）",
        "settingsConfig": { "auth": auth, "config": cfg },
        "category": "custom",
        "icon": "openai",
        "notes": "从 ~/.codex/ 导入。",
    }))
}

/// 取/建某个 app 段（ProviderManager: `{ providers:{}, current:"" }`），返回其可变对象。
fn ensure_app<'a>(
    obj: &'a mut serde_json::Map<String, Value>,
    app: &str,
) -> &'a mut serde_json::Map<String, Value> {
    let sec = obj
        .entry(app.to_string())
        .or_insert_with(|| json!({ "providers": {}, "current": "" }));
    if !sec.is_object() {
        *sec = json!({ "providers": {}, "current": "" });
    }
    let m = sec.as_object_mut().unwrap();
    if !m.get("providers").map(|v| v.is_object()).unwrap_or(false) {
        m.insert("providers".to_string(), json!({}));
    }
    if !m.contains_key("current") {
        m.insert("current".to_string(), json!(""));
    }
    m
}

/// 把一个 provider 塞进某 app 段（幂等，按 id 覆盖）；`set_current_if_empty` 且 current 为空时设当前。
fn put_provider(
    app_obj: &mut serde_json::Map<String, Value>,
    id: &str,
    provider: Value,
    set_current_if_empty: bool,
) {
    app_obj["providers"]
        .as_object_mut()
        .unwrap()
        .insert(id.to_string(), provider);
    if set_current_if_empty {
        let empty = app_obj
            .get("current")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .is_empty();
        if empty {
            app_obj.insert("current".to_string(), json!(id));
        }
    }
}

#[cfg(windows)]
const CC_NO_WINDOW: u32 = 0x0800_0000;

/// 便携 node（U-King 装的 `~/.uking/runtime/node`），否则系统 node。写 cc-switch 库时用它的
/// 内置 `node:sqlite`（node ≥22.5），**不给 U-King 加 rusqlite 重依赖**（体积优先）。
#[cfg(windows)]
fn find_node() -> String {
    #[cfg(windows)]
    let exe = "node.exe";
    #[cfg(not(windows))]
    let exe = "node";
    let cand = home_dir()
        .join(".uking")
        .join("runtime")
        .join("node")
        .join(exe);
    if cand.exists() {
        cand.to_string_lossy().into_owned()
    } else {
        exe.to_string()
    }
}

/// uu-switch(cc-switch.exe) 是否在运行（写库前要先关，库是 journal_mode=delete，开着写不进）。
#[cfg(windows)]
fn proc_running() -> bool {
    use std::os::windows::process::CommandExt;
    std::process::Command::new(crate::installer::system_tool("tasklist"))
        .args(["/FI", "IMAGENAME eq cc-switch.exe", "/NH"])
        .creation_flags(CC_NO_WINDOW)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("cc-switch.exe"))
        .unwrap_or(false)
}

/// 关闭 uu-switch：`force=false` 优雅关（给它落盘机会），`true` 强杀兜底。
#[cfg(windows)]
fn proc_close(force: bool) {
    use std::os::windows::process::CommandExt;
    let args: &[&str] = if force {
        &["/F", "/IM", "cc-switch.exe", "/T"]
    } else {
        &["/IM", "cc-switch.exe", "/T"]
    };
    let _ = std::process::Command::new(crate::installer::system_tool("taskkill"))
        .args(args)
        .creation_flags(CC_NO_WINDOW)
        .output();
}

/// 轮询等 uu-switch 完全退出，最多 `timeout_ms`。
#[cfg(windows)]
fn proc_wait_exited(timeout_ms: u64) -> bool {
    let mut waited = 0u64;
    while waited < timeout_ms {
        if !proc_running() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
        waited += 250;
    }
    !proc_running()
}

/// 写进 cc-switch SQLite 库的 node 脚本（内置 `node:sqlite`，`--experimental-sqlite` 启用）。
/// `INSERT OR REPLACE` 按主键 `(id, app_type)` —— **只增/替换我们那几条（uking-* id），主键
/// 不同的别人条目一条都不碰**。`is_current` 一律写 0：只新增选项，**绝不改用户当前选择**。
#[cfg(windows)]
const DB_IMPORT_JS: &str = r#"import { DatabaseSync } from "node:sqlite";
import { readFileSync } from "node:fs";
const [, , jsonPath, dbPath] = process.argv;
let rows;
try { rows = JSON.parse(readFileSync(jsonPath, "utf8")); }
catch (e) { process.stderr.write("read import data failed: " + e.message + "\n"); process.exit(2); }
let db;
try { db = new DatabaseSync(dbPath); }
catch (e) { process.stderr.write("open db failed: " + e.message + "\n"); process.exit(3); }
try {
  const stmt = db.prepare("INSERT OR REPLACE INTO providers (id,app_type,name,settings_config,website_url,category,created_at,sort_index,notes,icon,meta,is_current,in_failover_queue,cost_multiplier) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?)");
  let n = 0;
  for (const p of rows) {
    stmt.run(p.id, p.app_type, p.name, JSON.stringify(p.settingsConfig), p.websiteUrl || null, p.category || "custom", Date.now(), 999, p.notes || null, p.icon || null, "{}", 0, 0, "1.0");
    n++;
  }
  process.stdout.write(JSON.stringify({ ok: true, written: n }) + "\n");
} catch (e) {
  process.stderr.write("write db failed: " + e.message + "\n"); process.exit(4);
} finally { try { db.close(); } catch {} }
"#;

/// 构建要导入的 provider 列表：`(app_type, provider Value, 全新时是否设为当前)`。
/// 虾盘云 Claude+Codex（新用户设当前）+ 在用的 Claude/Codex 配置（不抢当前）。
fn build_import_providers(key: &str) -> Vec<(&'static str, Value, bool)> {
    let mut v = vec![
        ("claude", xiapan_claude_provider(key), true),
        ("codex", xiapan_codex_provider(key), true),
    ];
    if let Some(p) = current_claude_provider() {
        v.push(("claude", p, false));
    }
    if let Some(p) = current_codex_provider() {
        v.push(("codex", p, false));
    }
    v
}

/// 直接写 cc-switch 的 SQLite 库（老用户/已开过 uu-switch 也可靠生效）。流程 = 关 uu-switch →
/// 写库（node:sqlite，INSERT OR REPLACE 只动我们那几条）→ 原来开着就再打开。**绝不删/改别人的**。
#[cfg(windows)]
fn import_via_db(
    db_path: &Path,
    providers: &[(&'static str, Value, bool)],
    on_progress: &(dyn Fn(&str) + Send + Sync),
) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    // 行数据 = provider Value + app_type。
    let rows: Vec<Value> = providers
        .iter()
        .map(|(app, prov, _)| {
            let mut r = prov.clone();
            if let Some(o) = r.as_object_mut() {
                o.insert("app_type".to_string(), json!(app));
            }
            r
        })
        .collect();
    let rows_json = serde_json::to_string(&rows).map_err(|e| format!("序列化失败: {e}"))?;
    let tmp_json = std::env::temp_dir().join("uuswitch-import.json");
    let tmp_js = std::env::temp_dir().join("uuswitch-db-import.mjs");
    std::fs::write(&tmp_json, &rows_json).map_err(|e| format!("写临时数据失败: {e}"))?;
    std::fs::write(&tmp_js, DB_IMPORT_JS).map_err(|e| format!("写临时脚本失败: {e}"))?;

    // 库锁：uu-switch 开着写不进 → 先优雅关、超时强杀；记住原来开着没，写完再开。
    let was_running = proc_running();
    if was_running {
        on_progress("正在关闭 uu-switch 以写入配置…");
        proc_close(false);
        if !proc_wait_exited(4000) {
            proc_close(true);
            proc_wait_exited(2000);
        }
    }

    on_progress("正在把虾盘云写入 uu-switch 驱动列表…");
    let node = find_node();
    let out = std::process::Command::new(&node)
        .args([
            "--experimental-sqlite",
            &tmp_js.to_string_lossy(),
            &tmp_json.to_string_lossy(),
            &db_path.to_string_lossy(),
        ])
        .creation_flags(CC_NO_WINDOW)
        .output();
    let _ = std::fs::remove_file(&tmp_json);
    let _ = std::fs::remove_file(&tmp_js);

    // 原来开着才再打开（不主动替用户拉起）。
    if was_running {
        let _ = launch();
    }

    match out {
        Ok(o) if String::from_utf8_lossy(&o.stdout).contains("\"ok\":true") => Ok(()),
        Ok(o) => Err(format!(
            "写 uu-switch 驱动库失败：{}",
            String::from_utf8_lossy(&o.stderr)
                .lines()
                .last()
                .unwrap_or("未知错误")
        )),
        Err(e) => Err(format!(
            "启动 node 写库失败：{e}（需要 U-King 便携 node 或系统 node ≥22.5）"
        )),
    }
}

/// 非破坏式写 `~/.cc-switch/config.json`（cc-switch 首次打开、库还没建时会自动导入）。
/// 保留用户已有 provider / 其它段；`put_provider` 的 set_current 只在 current 为空时设，不覆盖。
fn import_via_config_json(
    path: &Path,
    providers: &[(&'static str, Value, bool)],
) -> Result<(), String> {
    let mut root: Value = if path.exists() {
        let s = std::fs::read_to_string(path).map_err(|e| format!("读 uu-switch 配置失败: {e}"))?;
        serde_json::from_str(&s).map_err(|_| {
            "uu-switch 配置不是合法 JSON，未改动。请先在 uu-switch 里正常保存一次再导入。".to_string()
        })?
    } else {
        json!({ "version": 2 })
    };
    let obj = root
        .as_object_mut()
        .ok_or("uu-switch 配置格式异常（顶层不是对象）")?;
    obj.insert("version".to_string(), json!(2));
    for (app, prov, set_current) in providers {
        let id = prov
            .get("id")
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .to_string();
        let sec = ensure_app(obj, app);
        put_provider(sec, &id, prov.clone(), *set_current);
    }
    // 备份 + 落盘。
    if path.exists() {
        let bak = PathBuf::from(format!("{}.uking-bak", path.to_string_lossy()));
        let _ = std::fs::copy(path, &bak);
    } else if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("建 uu-switch 配置目录失败: {e}"))?;
    }
    let pretty =
        serde_json::to_string_pretty(&root).map_err(|e| format!("序列化 uu-switch 配置失败: {e}"))?;
    std::fs::write(path, pretty.as_bytes()).map_err(|e| format!("写 uu-switch 配置失败: {e}"))?;
    Ok(())
}

/// 一键把「虾盘云（Claude + Codex）」+「你在用的工具配置（Claude / Codex）」导入 uu-switch。
///
/// **只增不删、绝不覆盖别人的**（用户明确要求 + cc-switch「绝不静默改用户配置」）：只写我们自己
/// 那几条（uking-* / uking-current-*），主键不同的用户条目**一条不碰**；写库时 `is_current` 一律
/// 不动（**不改用户当前选择**）。幂等：重复导入=更新同一条，不堆重复。
///
/// **两条路（cc-switch v3.18+ 已用 SQLite 库存 provider）**：
/// - **有库**（用户打开过 uu-switch）→ **直接写库**（关 uu-switch → node:sqlite 写 → 原来开着再开），
///   老用户也可靠生效；
/// - **没库**（还没打开过）→ 写 `config.json`（cc-switch 首次打开自动导入，导入后归档 .migrated）。
///
/// 值来自 U-King 单一真相源：设备 Key + 虾盘云端点 + deepseek-v4-pro（Claude）/ gpt-5.3-codex（Codex），
/// 与 U-King 自己配的口径一致 → 两侧切换等效。「在用配置」= 读 ~/.claude、~/.codex 原样搬（托管的跳过）。
pub fn import_to_uuswitch(
    on_progress: &(dyn Fn(&str) + Send + Sync),
) -> Result<String, String> {
    // 这条会写 uu-switch 的 SQLite 库 / config.json —— 又一处「改别人家的配置」。
    // 只增不覆盖是设计约定，但真出了「我的配置被改了」的投诉，得有记录能对质。
    crate::ulog::section("uuswitch", "导入虾盘云 + 在用配置到 uu-switch");
    let _ = &on_progress; // 仅 Windows 写库路径用到；非 Windows（无 uu-switch）走 config.json 不用它
    let key = crate::device::device_key_offline()?;
    let providers = build_import_providers(&key);

    // 人话汇总（写库/写 config.json 都用这份）。
    let mut done: Vec<&str> = vec!["虾盘云（Claude + Codex）"];
    let has = |id: &str| {
        providers
            .iter()
            .any(|(_, p, _)| p.get("id").and_then(|x| x.as_str()) == Some(id))
    };
    if has(CURRENT_CLAUDE_ID) {
        done.push("你在用的 Claude 配置");
    }
    if has(CURRENT_CODEX_ID) {
        done.push("你在用的 Codex 配置");
    }
    let items = done.join("、");

    let cfg_path = cc_switch_config_path();

    // 有库 = 直接写库（可靠，老用户也吃）。仅 Windows（uu-switch 只发 Windows 包）。
    #[cfg(windows)]
    {
        let db_path = cfg_path.with_file_name("cc-switch.db");
        if db_path.exists() {
            import_via_db(&db_path, &providers, on_progress)?;
            return Ok(format!(
                "已导入到 uu-switch：{items}（写进它的驱动库·只新增不覆盖）。打开 uu-switch 就能在列表里点它切换 —— 之后 U-King / uu-switch 任一侧切换等效。"
            ));
        }
    }

    // 没库（还没打开过 uu-switch）→ config.json，首次打开自动导入。
    import_via_config_json(&cfg_path, &providers)?;
    Ok(format!(
        "已准备好导入（{items}）。现在打开 uu-switch 会自动把它们导入到驱动列表 —— 之后 U-King / uu-switch 任一侧切换等效。"
    ))
}

/// 下载并**静默安装** uu-switch（MSI `msiexec /i /qn`，per-user 免管理员，~12 MB）。
/// 装完不改用户任何 AI 配置（cc-switch 哲学：切驱动是用户在 uu-switch 里主动做的事）。
/// 主源失败自动切 OSS 兜底；静默失败（被拦/取消）回退拉起可视安装界面。返回人话进度给前端 toast。
#[cfg(windows)]
pub fn install(on_progress: &(dyn Fn(&str) + Send + Sync)) -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    // 幂等：已装就别重下重装（对齐「已装」徽标口径 installed()）。
    if installed() {
        on_progress("检测到 uu-switch 已安装，跳过重复安装。");
        return Ok("uu-switch 已安装（跳过重复安装）。".into());
    }

    let tmp = std::env::temp_dir().join("uu-switch-Setup-uking.msi");
    let _ = std::fs::remove_file(&tmp);

    on_progress("开始下载 uu-switch（约 12 MB，网络好十几秒）…");
    // 主源（国内可达）失败 → OSS 兜底。curl 复用系统 curl.exe（Win10+ 内置）。
    // `-f`：HTTP ≥400 直接失败，别把 404 报错页当成功保成 .msi；`--retry`：抖动网络自动重试。
    // 每源开下前清掉上一源的半截文件；下完用 looks_like_msi 校验「完整且是真 MSI」才算成功
    //（size>3MB 的老判据太松：半截包也能过，结果坏包被喂给 msiexec 弹「无法打开此安装程序包」）。
    let mut ok = false;
    let mut last_sz: u64 = 0;
    for url in [UUSWITCH_URL_CN, UUSWITCH_URL_OSS] {
        let _ = std::fs::remove_file(&tmp);
        let status = std::process::Command::new(crate::installer::system_tool("curl"))
            .args([
                "-fsSL",
                "--retry",
                "2",
                "--retry-delay",
                "1",
                "-A",
                "Mozilla/5.0 U-King",
                "-m",
                "240",
                "-o",
                &tmp.to_string_lossy(),
                url,
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .status();
        last_sz = std::fs::metadata(&tmp).map(|m| m.len()).unwrap_or(0);
        if matches!(status, Ok(s) if s.success()) && looks_like_msi(&tmp) {
            ok = true;
            break;
        }
        on_progress("主源下载不畅，正在尝试备用源…");
    }
    if !ok {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!(
            "uu-switch 安装包下载不完整或被拦截（已下 {last_sz} 字节，可能是网络/代理/杀软所致）。\
             请在工具卡点「打开下载页」手动下载安装。"
        ));
    }
    on_progress("下载完成，正在安装 uu-switch…");

    // 落盘→执行之间可能被杀软掏空/隔离，交给 msiexec 前再校一次（成本可忽略）。坏了就人话报错，
    // 绝不把坏包塞给 msiexec（那会弹「无法打开此安装程序包」MSI 1620 的吓人系统框）。
    if !looks_like_msi(&tmp) {
        let _ = std::fs::remove_file(&tmp);
        return Err(
            "安装包落盘后被改动（多半被杀软拦截/隔离）。请在工具卡点「打开下载页」手动安装。".into(),
        );
    }

    // MSI 静默安装（per-user，无需管理员）：msiexec /i <msi> /qn /norestart。
    // msiexec 绝对路径走 system_tool，防客户机 PATH 被改坏丢 System32。
    let out = std::process::Command::new(crate::installer::system_tool("msiexec"))
        .args(["/i", &tmp.to_string_lossy(), "/qn", "/norestart"])
        .creation_flags(CREATE_NO_WINDOW)
        .status();
    // 回退到「可视安装界面」的前提：文件仍是完整 MSI（挡住 1620/1619 这类「包本身坏」的场景，
    // 否则可视 msiexec 照样弹「无法打开此安装程序包」——那就白回退了）。只在包好、纯粹被拦/被占/
    // 用户取消（1602/1603/1618）时才拉起 UI 让用户手动点下一步。
    let visible_fallback = |code: String| -> String {
        if looks_like_msi(&tmp) {
            on_progress("静默安装未成，已打开安装界面，按提示点「下一步」即可…");
            let _ = std::process::Command::new(crate::installer::system_tool("msiexec"))
                .args(["/i", &tmp.to_string_lossy()])
                .creation_flags(CREATE_NO_WINDOW)
                .spawn();
            format!("已打开 uu-switch 安装程序（{code}），按提示装完即可。")
        } else {
            let _ = std::fs::remove_file(&tmp);
            "安装包在安装过程中被破坏（多半被杀软拦截）。请在工具卡点「打开下载页」手动安装。".into()
        }
    };
    match out {
        Ok(s) => {
            let code = s.code().unwrap_or(-1);
            // 0=成功；3010=成功待重启。都算装好；MSI 是事务性，返回即已落文件。
            if code == 0 || code == 3010 {
                for _ in 0..10 {
                    if installed() {
                        let _ = std::fs::remove_file(&tmp);
                        return Ok("uu-switch 已安装完成，可在应用列表打开。".into());
                    }
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }
                let _ = std::fs::remove_file(&tmp);
                return Ok("uu-switch 安装已完成（若列表未刷新，稍等片刻）。".into());
            }
            // 静默失败（1602 用户取消 / 1603 致命 / 1618 有安装在跑）→ 回退可视安装界面（前提：包仍完整）。
            Ok(visible_fallback(format!("安装码 {code}")))
        }
        Err(e) => Ok(visible_fallback(e.to_string())),
    }
}

#[cfg(not(windows))]
pub fn install(_on_progress: &(dyn Fn(&str) + Send + Sync)) -> Result<String, String> {
    Err("当前平台请到工具卡点「打开下载页」安装 uu-switch".into())
}

/// 启动已装的 uu-switch（GUI 应用，找安装位置直接拉起，不进终端）。
#[cfg(windows)]
pub fn launch() -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let exe = find_exe().ok_or("未找到 uu-switch（请先安装 uu-switch）")?;
    std::process::Command::new(&exe)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("启动 uu-switch 失败: {e}"))?;
    Ok(())
}

#[cfg(not(windows))]
pub fn launch() -> Result<(), String> {
    std::process::Command::new("open")
        .args(["-a", "uu-switch"])
        .spawn()
        .map_err(|e| format!("启动 uu-switch 失败: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod uuswitch_config_tests {
    use super::*;

    /// 导给 uu-switch 的 Codex 配置，模型必须和 U-King 自己写的一致。
    ///
    /// 这条钉的是一个**只在客户账单上才看得见**的 bug：这里曾写死 `gpt-5.3-codex`，
    /// 而 U-King 自己默认走便宜的 `deepseek-v4-flash-codex`。客户装了 uu-switch、
    /// 在那边点一下切换，就被静默换成贵几十倍的模型 —— 界面上什么都不会提示他。
    #[test]
    fn imported_codex_config_matches_uking_default_model() {
        let want = crate::providers::xiapan_codex_model();
        let v = xiapan_codex_provider("sk-test-dummy");
        let cfg = v["settingsConfig"]["config"].as_str().unwrap_or_default();
        assert!(
            cfg.contains(&format!("model = \"{want}\"")),
            "uu-switch 导入的模型和 U-King 预设漂移了：期望 {want}，实际配置为\n{cfg}"
        );
        assert!(!cfg.contains("gpt-5.3-codex"), "别再把贵几十倍的模型写死进去");
        // 备注也给客户看，一起对上，否则「写着 A 跑着 B」
        assert!(v["notes"].as_str().unwrap_or_default().contains(&want));
    }
}
