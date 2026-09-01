//! 被管理契约（企业版第一层）—— 「这台机器归谁管」的身份落盘。
//!
//! ## 独立可插拔（宪法第 12 条）
//! 本模块不认识 `AppHandle`，不 import 任何功能模块，纯 std + serde_json。
//! **删掉本模块只动两处**：`lib.rs`（去 mod + 动作登记）。前端本切片零改动。
//!
//! ## 企业个人都能用 = 默认 unmanaged
//! `~/.uking/org.json` 默认 `mode:"unmanaged"`，此时本模块对个人版行为**零影响**。
//! 企业把机器 enroll 成 `managed` 后，也只多出一份身份记录，**不自动做任何事**——
//! 策略下发 / 遥测回流是后续步骤（需求榜 E2/E3），且遥测回流必须显式联动
//! `metrics` consent，绝不因为「是 managed」就擅自把个人数据发出去（红线）。
//!
//! ## 写动作只登记幂等（协议铁律）
//! `enroll` / `disenroll` 天然幂等：同样入参重放结果一样，重放安全靠幂等不靠账本。

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

/// `~/.uking`。**认 `UKING_TEST_HOME` 沙箱**（同 automation.rs / providers.rs）——
/// 开发机上验 enroll/disenroll 必须能跑，又不能碰真实的 `~/.uking/org.json`。
fn uking_home() -> PathBuf {
    let home = std::env::var("UKING_TEST_HOME")
        .ok()
        .filter(|t| !t.is_empty())
        .or_else(|| std::env::var("USERPROFILE").ok())
        .or_else(|| std::env::var("HOME").ok())
        .unwrap_or_else(|| ".".into());
    PathBuf::from(home).join(".uking")
}

fn org_path() -> PathBuf {
    uking_home().join("org.json")
}

/// 读写锁：enroll/disenroll 可能和 GUI / 未来总控台同时写，别让它们把对方覆盖掉。
fn lock() -> &'static Mutex<()> {
    static L: OnceLock<Mutex<()>> = OnceLock::new();
    L.get_or_init(|| Mutex::new(()))
}

/// 企业托管身份。默认 `unmanaged` —— 个人版从这里读到的永远是「没被托管」。
#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct OrgConfig {
    /// 结构版本，将来加字段时升号迁移。
    pub schema: u32,
    /// "unmanaged"（个人版，默认）| "managed"（已被某企业托管）。
    pub mode: String,
    /// 企业分配的组织标识（enroll 必填）。
    pub org_id: Option<String>,
    /// 展示名（可选）。
    pub org_name: Option<String>,
    /// 策略下发端点（可选，本切片只用记录；需求榜 E2 用它拉策略）。
    pub policy_url: Option<String>,
    /// 登记时间（unix 秒）。
    pub enrolled_at: Option<i64>,
}

impl Default for OrgConfig {
    fn default() -> Self {
        Self {
            schema: 1,
            mode: "unmanaged".into(),
            org_id: None,
            org_name: None,
            policy_url: None,
            enrolled_at: None,
        }
    }
}

fn read() -> OrgConfig {
    std::fs::read_to_string(org_path())
        .ok()
        .and_then(|s| serde_json::from_str::<OrgConfig>(&s).ok())
        .unwrap_or_default()
}

fn write(c: &OrgConfig) -> Result<(), String> {
    let _ = std::fs::create_dir_all(uking_home());
    let s = serde_json::to_string_pretty(c).map_err(|e| format!("序列化 org.json 失败: {e}"))?;
    std::fs::write(org_path(), s).map_err(|e| format!("写入 org.json 失败: {e}"))
}

/// 只读动作 `runtime.org.inspect` 的输出形状。
///
/// `ready = (mode == "managed")`：回答「这台机器**在被管吗**」而不是「能不能被管」——
/// 总控台要的就是这个；conformance 会把 unmanaged 汇总进 `not_ready`（事实，不是 bug）。
pub fn inspect_json() -> Value {
    let c = read();
    let managed = c.mode == "managed";
    let mut blockers = Vec::new();
    if !managed {
        blockers.push("当前未加入企业托管（个人版模式，数据不出机）".to_string());
    }
    json!({
        "mode": c.mode,
        "org_id": c.org_id,
        "org_name": c.org_name,
        "policy_url": c.policy_url,
        "enrolled_at": c.enrolled_at,
        "ready": managed,
        "blockers": blockers,
    })
}

/// 登记进某企业。**幂等**：已 managed 且 org_id 相同 → 不重复写，原样返回当前状态。
pub fn enroll(
    org_id: &str,
    org_name: Option<&str>,
    policy_url: Option<&str>,
) -> Result<Value, String> {
    let org_id = org_id.trim();
    if org_id.is_empty() {
        return Err("org_id 不能为空".into());
    }
    let _g = lock().lock().unwrap_or_else(|e| e.into_inner());
    let cur = read();
    if cur.mode == "managed" && cur.org_id.as_deref() == Some(org_id) {
        // 幂等重放：同样的入参结果一样，不重复写。
        return Ok(inspect_json());
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let c = OrgConfig {
        schema: 1,
        mode: "managed".into(),
        org_id: Some(org_id.to_string()),
        org_name: org_name.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
        policy_url: policy_url
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        enrolled_at: Some(now),
    };
    write(&c)?;
    Ok(inspect_json())
}

/// 退出企业托管，还原成个人版。**幂等**：已 unmanaged → 不重复写，原样返回。
pub fn disenroll() -> Result<Value, String> {
    let _g = lock().lock().unwrap_or_else(|e| e.into_inner());
    if read().mode != "managed" {
        return Ok(inspect_json());
    }
    write(&OrgConfig::default())?;
    Ok(inspect_json())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 每个用例一个独立沙箱（`UKING_TEST_HOME`），绝不碰开发机真实的 ~/.uking。
    ///
    /// 🔴 这里曾经有一把**本地** `SANDBOX_LOCK` —— 从 providers.rs 连注释一起复制过来的，
    /// 注释还写着「串行跑：共用同一个进程级环境变量」，看着完全正确，其实锁的不是同一个
    /// 对象，跟别的模块并行时照样互踩（`enroll_then_inspect_is_managed` 稳定读到
    /// `unmanaged`）。锁只能有一把，在 `crate::testsandbox`。
    fn with_sandbox(tag: &str, f: impl FnOnce()) {
        crate::testsandbox::with_sandbox(&format!("org-{tag}"), &[".uking"], |_| f())
    }

    #[test]
    fn default_is_unmanaged() {
        with_sandbox("default", || {
            let j = inspect_json();
            assert_eq!(j["mode"], "unmanaged");
            assert_eq!(j["ready"], false);
            assert!(j["org_id"].is_null());
        });
    }

    #[test]
    fn enroll_then_inspect_is_managed() {
        with_sandbox("enroll", || {
            enroll(
                "demo-org",
                Some("测试企业"),
                Some("https://example.com/policy.json"),
            )
            .unwrap();
            let j = inspect_json();
            assert_eq!(j["mode"], "managed");
            assert_eq!(j["org_id"], "demo-org");
            assert_eq!(j["org_name"], "测试企业");
            assert_eq!(j["policy_url"], "https://example.com/policy.json");
            assert_eq!(j["ready"], true);
            assert!(j["enrolled_at"].as_i64().unwrap() > 0);
        });
    }

    #[test]
    fn enroll_same_org_is_idempotent() {
        with_sandbox("idempotent", || {
            enroll("demo-org", None, None).unwrap();
            let first = inspect_json();
            // 同 org 重放：不重复写，enrolled_at 不变。
            std::thread::sleep(std::time::Duration::from_millis(5));
            enroll("demo-org", None, None).unwrap();
            let second = inspect_json();
            assert_eq!(first["enrolled_at"], second["enrolled_at"]);
        });
    }

    #[test]
    fn enroll_empty_org_id_rejected() {
        with_sandbox("empty", || {
            assert!(enroll("   ", None, None).is_err());
        });
    }

    #[test]
    fn disenroll_restores_unmanaged() {
        with_sandbox("disenroll", || {
            enroll("demo-org", None, None).unwrap();
            disenroll().unwrap();
            let j = inspect_json();
            assert_eq!(j["mode"], "unmanaged");
            assert_eq!(j["ready"], false);
            assert!(j["org_id"].is_null());
        });
    }

    #[test]
    fn disenroll_when_already_unmanaged_is_idempotent() {
        with_sandbox("disenroll-idem", || {
            assert!(disenroll().is_ok());
            let j = inspect_json();
            assert_eq!(j["mode"], "unmanaged");
        });
    }

    #[test]
    fn sandbox_resolves_into_temp_dir_not_real_home() {
        with_sandbox("isolation", || {
            enroll("demo-org", None, None).unwrap();
            // org_path 必须落在沙箱临时目录里，而不是开发机真实的 ~/.uking。
            // 只认 tag（`org-isolation`）不认整个目录名前缀 —— 前缀归 `testsandbox` 管，
            // 钉死它等于把公共层的实现细节焊进这条断言里。
            let p = org_path().to_string_lossy().to_string();
            assert!(p.contains("org-isolation"), "org_path 竟落在真实 home：{p}");
        });
    }
}
