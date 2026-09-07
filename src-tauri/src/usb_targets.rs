//! Target manifest —— 便携 AI「目标运行时」的声明式描述（合流方案 P2）。
//!
//! 真相源：`docs/uclaw-genie-convergence.md`（opus 评审稿）。要点：
//! - manifest **只从 U-King 包内加载**（`include_str!`），U 盘上的文件永远不是
//!   可信输入 —— 这条是安全红线，不是工程取舍；
//! - `kind` 是闭集（`openclaw | picoclaw`），编译期穷举，不认识直接拒；
//! - `kind == openclaw` 强制 `config.transport == http-api`，堵死任何「直接写
//!   openclaw.json」的路（有编译期测试断言兜底，见 `compile_time_guards`）；
//! - 新增第三个 target 只加一份 manifest JSON，**不新增 action**：注册表长度
//!   恒定，action-parity 的静态可枚举性不受影响。
//!
//! v1 刻意薄：只有 P1 已验证过的 picoclaw 一份 manifest。U-Claw 的 http-api
//! transport 属于 P3，schema 先留好位（`ConfigTransport::HttpApi` 变体已存在），
//! 但解释器暂不对它开放。

use std::fmt;

/// 运行时种类。闭集：出现新种类先在这里加变体、再过编译期穷举断言。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetKind {
    Picoclaw,
    Openclaw,
}

impl TargetKind {
    pub fn as_str(self) -> &'static str {
        match self {
            TargetKind::Picoclaw => "picoclaw",
            TargetKind::Openclaw => "openclaw",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigTransport {
    /// PicoClaw 现状：U-King 直接管理盘上的 config.json / .security.yml
    /// （usb_genie.rs 已有的形状锁与原子写）。
    File,
    /// P3 预留：U-Claw 走 config-server 的 JSON API；壳绝不直接写
    /// openclaw.json（u-claw 仓 docs/config-server-api.md 是契约真相源）。
    HttpApi,
}

impl ConfigTransport {
    pub fn as_str(self) -> &'static str {
        match self {
            ConfigTransport::File => "file",
            ConfigTransport::HttpApi => "http-api",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthType {
    Process,
    Http,
}

impl HealthType {
    pub fn as_str(self) -> &'static str {
        match self {
            HealthType::Process => "process",
            HealthType::Http => "http",
        }
    }
}

/// 一份 target manifest 的校验后视图。字段与
/// `src-tauri/resources/targets/picoclaw.manifest.json` 一一对应。
#[derive(Debug, Clone)]
pub struct TargetManifest {
    pub id: &'static str,
    pub display_name: &'static str,
    pub tagline: &'static str,
    pub kind: TargetKind,
    /// 已实现能力面。v1 的 picoclaw 没有 stop/status/config —— 硬扩会虚报，
    /// 等对应 action 真落地再扩（ honesty first：UI 按这个数组渲染按钮）。
    pub capabilities: &'static [&'static str],
    /// 盘上程序子树（相对 U 盘根，正斜杠书写，跨平台比较时统一）。
    pub program_dir: &'static str,
    /// 判「已安装」所需的文件（相对 program_dir）。
    pub installed_markers: &'static [&'static str],
    pub config_transport: ConfigTransport,
    pub health: HealthType,
    /// U 盘凭据为明文的诚实声明。红线：恒为 true，不可被 manifest 关掉。
    pub credential_plaintext: bool,
}

impl TargetManifest {
    /// 校验 + 构造。`parse` 层保证 JSON 形状，这里保证语义不变式；
    /// 任何违反都让构建/测试当场炸，而不是上线后静默漂移。
    fn validate(m: TargetManifest) -> TargetManifest {
        assert!(
            matches!(m.kind, TargetKind::Picoclaw | TargetKind::Openclaw),
            "manifest {} 的 kind 不在闭集内",
            m.id
        );
        // 红线（方案 §3.4 编译期断言）：openclaw 一律走 http-api，堵死直写 openclaw.json。
        assert!(
            m.kind != TargetKind::Openclaw || m.config_transport == ConfigTransport::HttpApi,
            "manifest {} 是 openclaw 但 transport 不是 http-api：直写 openclaw.json 被架构禁止",
            m.id
        );
        assert!(
            m.kind != TargetKind::Picoclaw || m.config_transport == ConfigTransport::File,
            "manifest {} 是 picoclaw 但 transport 不是 file",
            m.id
        );
        // 红线：U 盘凭据明文声明不可关。
        assert!(
            m.credential_plaintext,
            "manifest {} 关掉了 credential_plaintext：明文诚实声明是红线，不可关闭",
            m.id
        );
        assert!(!m.capabilities.is_empty(), "manifest {} 没有任何 capability", m.id);
        for cap in m.capabilities {
            assert!(
                matches!(*cap, "detect" | "start" | "stop" | "status" | "config"),
                "manifest {} 的 capability {cap} 不在已知集合",
                m.id
            );
        }
        m
    }
}

/// 包内内置 manifest 表。加第三个 target = 在这里多一行。故意不用运行时
/// dir-read 动态发现：manifest 表必须静态可枚举，和 action 注册表同构；
/// `OnceLock` 让 validate 只跑一次、表以 `&'static` 出借。
pub fn builtin_manifests() -> &'static [TargetManifest] {
    static TABLE: std::sync::OnceLock<Vec<TargetManifest>> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        vec![TargetManifest::validate(TargetManifest {
            id: "picoclaw",
            display_name: "AI 精灵 (PicoClaw)",
            tagline: "轻量 · 开箱即用 · 离线可对话",
            kind: TargetKind::Picoclaw,
            capabilities: &["detect", "start", "config"],
            program_dir: "U-King/AI-Genie",
            installed_markers: &[
                "current.json",
                "runtime/current/picoclaw.exe",
                "data/config.json",
            ],
            config_transport: ConfigTransport::File,
            health: HealthType::Process,
            credential_plaintext: true,
        })]
    })
}

/// 按 id 取 manifest。`target.*` action 的 `id` 入参只认这张表。
pub fn by_id(id: &str) -> Option<&'static TargetManifest> {
    builtin_manifests().iter().find(|m| m.id == id)
}

/// JSON 形状的伴生导出（给 target.manifest.list / UI 用）。
/// 故意手写而不是 serde 序列化 struct：避免给 exe 添 serde_json 之外的依赖面，
/// 也让「manifest 表里有什么」在这个文件里一眼可读。
pub fn manifest_json(m: &TargetManifest) -> serde_json::Value {
    serde_json::json!({
        "id": m.id,
        "display_name": m.display_name,
        "tagline": m.tagline,
        "kind": m.kind.as_str(),
        "capabilities": m.capabilities,
        "program_dir": m.program_dir,
        "installed_markers": m.installed_markers,
        "config_transport": m.config_transport.as_str(),
        "health": m.health.as_str(),
        "credential_plaintext": m.credential_plaintext,
    })
}

impl fmt::Display for TargetKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ———————— action 实现层（P2：委托现有 usb_genie 链路，零行为变化） ————————
//
// 设计纪律（方案 §2 P2）：抽象层是「搬家」不是「改行为」。picoclaw 的每个
// target.* handler 都原样转调 usb_genie 对应实现；U 盘探测、凭据形状锁、
// launch 去重、inventory_state_version 乐观并发等语义全部继承，一行不改。
// U-Claw（http-api transport）属于 P3 —— 在此之前 handler 对非 picoclaw
// manifest 一律明确报 unsupported，不虚报能力。

use serde_json::{json, Value};

/// target.list → 检出的盘 × 内置 manifest 的笛卡尔积里「已安装」的条目。
/// 未装任何 target 的盘也会出现在 targets 里（installed=false），
/// 与 usb_genie::inspect 的形状保持同构，UI 迁移是字段改名不是重写。
pub fn action_target_list() -> Result<Value, String> {
    let manifests = builtin_manifests();
    // 盘面探测只有 usb_genie 一个真相源（同一事实只查一次）。
    let inspection = crate::usb_genie::inspect()?;
    let mut targets = Vec::new();
    for disk in inspection["targets"].as_array().cloned().unwrap_or_default() {
        let target_root = disk["target_root"].as_str().unwrap_or_default().to_string();
        let target_id = disk["target_id"].as_str().unwrap_or_default().to_string();
        for m in manifests {
            // v1：manifest 的 program_dir 与 usb_genie 的安装子树相同
            //（"U-King/AI-Genie"），installed 判据直接复用盘上记录。
            let installed = disk["installed"].as_bool().unwrap_or(false);
            targets.push(json!({
                "target_id": target_id,
                "target_root": target_root,
                "manifest_id": m.id,
                "display_name": m.display_name,
                "tagline": m.tagline,
                "kind": m.kind.as_str(),
                "capabilities": m.capabilities,
                "filesystem": disk["filesystem"],
                "free_bytes": disk["free_bytes"],
                "total_bytes": disk["total_bytes"],
                "installed": installed,
                "version": disk["picoclaw_version"],
                "credential_present": disk["credential_present"],
                "credential_plaintext": m.credential_plaintext,
                "target_state_version": disk["target_state_version"],
            }));
        }
    }
    Ok(json!({
        "targets": targets,
        "blockers": inspection["blockers"],
        "ready": inspection["ready"],
        "inventory_state_version": inspection["inventory_state_version"],
        "state_version": inspection["state_version"],
    }))
}

/// 通用入参解析：id 必须在内置表里；target_root/target_id 交给 usb_genie
/// 的盘面校验（它认的是「当前真实可移动盘」，不是调用方说了算）。
fn require_manifest(input: &Value) -> Result<&'static TargetManifest, String> {
    let id = input
        .get("id")
        .and_then(Value::as_str)
        .ok_or("invalid_input: id 必填（target manifest id，如 \"picoclaw\"）")?;
    crate::usb_targets::by_id(id)
        .ok_or_else(|| format!("unknown_target: {id} 不在 U-King 内置 target manifest 表内"))
}

/// target.manifest.list —— 调试/自检/UI 渲染表单用。只读包内表，不碰盘。
pub fn action_manifest_list() -> Result<Value, String> {
    Ok(json!({
        "manifests": builtin_manifests().iter().map(manifest_json).collect::<Vec<_>>(),
    }))
}

/// target.detect {disk?} —— 单盘扫描；不传 disk 就是全量快照。
/// 委托 usb_genie::inspect（它本来就是 removable-only、绝不递归扫盘）。
pub fn action_target_detect(input: &Value) -> Result<Value, String> {
    let _ = input.get("disk"); // v1 全量快照已含每盘信息；disk 参数留位，P3 单盘优化时用
    action_target_list()
}

/// P3 桩位共用：校验 manifest id 合法后，明确拒绝执行。
/// 拒绝前仍做 manifest 校验——打错 id 得到 unknown_target 而不是误导性的 unsupported。
pub fn action_target_unsupported(input: &Value) -> Result<Value, String> {
    let manifest = require_manifest(input)?;
    Err(format!(
        "unsupported_target: {0}（{1}）尚无此能力 —— 见 target.manifest.list 的 capabilities",
        manifest.id, manifest.display_name
    ))
}

fn delegated_target_input(input: &Value) -> Result<(String, String), String> {
    let manifest = require_manifest(input)?;
    if manifest.kind != TargetKind::Picoclaw {
        return Err(format!(
            "unsupported_target: {0}（{1}）尚未接入 —— 其 transport 属于后续阶段",
            manifest.id, manifest.display_name
        ));
    }
    let target_root = input
        .get("target_root")
        .and_then(Value::as_str)
        .ok_or("invalid_input: target_root 必填")?
        .to_string();
    let target_id = input
        .get("target_id")
        .and_then(Value::as_str)
        .ok_or("invalid_input: target_id 必填")?
        .to_string();
    Ok((target_id, target_root))
}

/// target.start {id, target_id, target_root} —— 委托 usb_genie::action_launch
/// （含 verify-before-launch 与 picoclaw 进程去重）。
pub fn action_target_start(
    _action_id: &str,
    input: Value,
    progress: &crate::actions::ProgressSink,
) -> Result<Value, String> {
    let (target_id, target_root) = delegated_target_input(&input)?;
    crate::usb_genie::action_launch(
        crate::actions::USB_GENIE_LAUNCH,
        json!({ "target_id": target_id, "target_root": target_root }),
        progress,
    )
}

/// target.status {id, target_id, target_root} —— picoclaw 现状 = verify()
/// 的完整七项检查（它就是 P1 的「状态」语义：装没装 + 好不好）。
pub fn action_target_status(
    _action_id: &str,
    input: Value,
    _progress: &crate::actions::ProgressSink,
) -> Result<Value, String> {
    let (target_id, target_root) = delegated_target_input(&input)?;
    crate::usb_genie::action_verify(
        crate::actions::USB_GENIE_VERIFY,
        json!({ "target_id": target_id, "target_root": target_root }),
        _progress,
    )
}

#[cfg(test)]
mod action_tests {
    use super::*;

    #[test]
    fn unknown_manifest_id_is_rejected() {
        let err = require_manifest(&json!({ "id": "not-a-target" })).unwrap_err();
        assert!(err.starts_with("unknown_target:"), "got: {err}");
    }

    #[test]
    fn missing_id_is_invalid_input() {
        let err = require_manifest(&json!({})).unwrap_err();
        assert!(err.starts_with("invalid_input:"), "got: {err}");
    }

    #[test]
    fn picoclaw_manifest_resolves() {
        let m = require_manifest(&json!({ "id": "picoclaw" })).unwrap();
        assert_eq!(m.kind, TargetKind::Picoclaw);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 方案 §3.4：编译期断言写成测试，不是约定。
    #[test]
    fn openclaw_requires_http_api_transport() {
        let result = std::panic::catch_unwind(|| {
            TargetManifest::validate(TargetManifest {
                id: "bad-openclaw",
                display_name: "bad",
                tagline: "bad",
                kind: TargetKind::Openclaw,
                capabilities: &["detect", "start"],
                program_dir: "U-Claw/portable",
                installed_markers: &[],
                config_transport: ConfigTransport::File,
                health: HealthType::Http,
                credential_plaintext: true,
            })
        });
        assert!(result.is_err(), "openclaw + file transport 必须在构造期被拒");
    }

    #[test]
    fn picoclaw_requires_file_transport() {
        let result = std::panic::catch_unwind(|| {
            TargetManifest::validate(TargetManifest {
                id: "bad-picoclaw",
                display_name: "bad",
                tagline: "bad",
                kind: TargetKind::Picoclaw,
                capabilities: &["detect"],
                program_dir: "U-King/AI-Genie",
                installed_markers: &[],
                config_transport: ConfigTransport::HttpApi,
                health: HealthType::Process,
                credential_plaintext: true,
            })
        });
        assert!(result.is_err(), "picoclaw + http-api transport 必须在构造期被拒");
    }

    #[test]
    fn credential_plaintext_cannot_be_disabled() {
        let result = std::panic::catch_unwind(|| {
            TargetManifest::validate(TargetManifest {
                id: "sneaky",
                display_name: "sneaky",
                tagline: "sneaky",
                kind: TargetKind::Picoclaw,
                capabilities: &["detect"],
                program_dir: "X",
                installed_markers: &[],
                config_transport: ConfigTransport::File,
                health: HealthType::Process,
                credential_plaintext: false,
            })
        });
        assert!(result.is_err(), "关闭明文声明必须在构造期被拒");
    }

    #[test]
    fn builtin_table_is_static_and_honest() {
        let manifests = builtin_manifests();
        // v1 恰好一份：picoclaw。加 U-Claw 时这里改 2 —— 这行断言就是「改表要过测试」的闸。
        assert_eq!(manifests.len(), 1, "v1 manifest 表应恰好一份 picoclaw");
        let picoclaw = by_id("picoclaw").expect("picoclaw manifest 必须在表内");
        assert_eq!(picoclaw.kind, TargetKind::Picoclaw);
        assert_eq!(picoclaw.config_transport, ConfigTransport::File);
        assert!(picoclaw.credential_plaintext);
        // capabilities 只声明真实现了的：P2 的 picoclaw 委托 usb_genie，没有 stop/status。
        assert_eq!(picoclaw.capabilities, &["detect", "start", "config"]);
        assert!(by_id("u-claw").is_none(), "u-claw 属于 P3，不该提前出现在表里");
    }

    #[test]
    fn manifest_json_roundtrip_shape() {
        let value = manifest_json(by_id("picoclaw").unwrap());
        assert_eq!(value["id"], "picoclaw");
        assert_eq!(value["kind"], "picoclaw");
        assert_eq!(value["config_transport"], "file");
        assert_eq!(value["health"], "process");
        assert_eq!(value["credential_plaintext"], true);
        assert!(value["capabilities"].as_array().unwrap().len() >= 1);
        // install markers 与 usb_genie.rs 的 PortableTarget::installed() 判据一致。
        assert!(value["installed_markers"].as_array().unwrap().len() == 3);
    }
}
