//! 设备身份与访问凭证 —— 「装上 U-King 就自带虾盘云账号，充值即用」。
//!
//! ## 现在的规则（设备钱包）
//!
//! 凭证由**服务端随机生成**（`/device/bind`），落在 `~/.uking/device.json`。
//! 机器指纹只用来做两件事：给服务端当风控线索，以及**认领老客户的钱包**。
//!
//! ```text
//! 首启 → 有没有 device.json？
//!        ├─ 有，且 keyKind=random → 完事（还有 pendingKey 就先把上次没走完的收尾）
//!        └─ 没有 / keyKind=fingerprint
//!             → 算出指纹 key，POST /device/migrate
//!                ├─ 200 → 拿到新凭证 → 验通 → commit（指纹 key 从此永久作废）
//!                ├─ 404 → 这台机器从没激活过 → POST /device/bind 建新钱包
//!                ├─ 409 → 迁过了但本地配置丢了 → 建新钱包 + 标记「老余额要凭订单号找回」
//!                └─ 端点没部署 → 退回老的指纹+激活路径
//! ```
//!
//! ## 为什么不再用「算出来的 key」当凭证
//!
//! 老规则是 key = `sk-` + sha256("uking|" + MachineGuid)，本地纯算、同机恒定。
//! 它有两个改不掉的毛病：
//!
//! - **换不掉。** 客户端每次都从硬件重算出同一把 key。某个客户 key 泄露了，
//!   我们没有任何补救手段 —— 只能让他换电脑。
//! - **MachineGuid 不是秘密。** 任何本地进程都读得到（`reg query` 不需要管理员）。
//!   闭源时这靠「没人愿意逆 exe」，开源之后是复制粘贴。
//!
//! ## 🔴 这里有个不能装作解决了的窗口
//!
//! 老客户**升级之前**，谁先拿到他的 MachineGuid、谁先调 migrate，钱包就归谁。
//! 无账号体系下服务端分不出「本人」和「知道他 MachineGuid 的人」。
//! 注意这不是新增的风险 —— 今天任何人拿到那把指纹 key 就能直接把余额花光；
//! 迁移只是把「以后」堵上。争议只能凭充值订单号人工裁定。
//!
//! ## 老路仍然留着
//!
//! `legacy_activate_fallback` 是完整的旧流程（指纹 key + `/recharge/activate`）。
//! **客户端可能比服务端先发出去**，那时 `/device/*` 全是 404，退不回去就等于
//! 新版一装上就没 key。等存量升完再删这段。
//!
//! 纯 std，零外部依赖：MachineGuid 用 reg query 读（失败退环境标识兜底），SHA256 用
//! **进程内纯 Rust 实现**算（不再依赖 PowerShell/shasum —— 客户机 PowerShell 损坏时
//! 也能拿到 Key）。

use serde::Serialize;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};

use crate::providers::{
    self, query_balance, Balance, MigrateOutcome, DEVICE_API_NOT_DEPLOYED,
};

/// device.json 里 `keyKind` 的两个取值。
const KIND_RANDOM: &str = "random";
const KIND_FINGERPRINT: &str = "fingerprint";

/// `pendingKind`：没走完的那一步是轮换还是迁移。收尾时要打不同的 commit 接口。
const PENDING_ROTATE: &str = "rotate";
const PENDING_MIGRATE: &str = "migrate";

/// 所有钱包收敛 / 换 Key / 导入 / 删除共用一把进程锁，避免并发首启重复建钱包。
static WALLET_OPERATION_LOCK: Mutex<()> = Mutex::new(());

fn wallet_operation_guard() -> MutexGuard<'static, ()> {
    WALLET_OPERATION_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[derive(Debug, Clone, Serialize)]
pub struct DeviceKey {
    /// sk- 开头的 64 位 hex Key
    pub key: String,
    /// 充值页（已带 key 参数）
    pub recharge_url: String,
    /// 余额（None = 还没充值 / 查询失败）
    pub balance: Option<Balance>,
    /// 服务端已有这个号（已充值过）
    pub charged: bool,
    /// 🔴 余额 >0 但**可能不够垫一次请求**。
    ///
    /// 上游发请求前要按「最多可能用掉多少」预扣一笔。Claude Code 上下文大，
    /// 单次预扣实测要 ¥0.358（客户 2026-08-18 的原话：
    /// `403 token quota is not enough, token remain quota: ¥0.340274, need quota: ¥0.358240`）。
    /// 于是余额剩三毛多的老客户**一条都发不出去**，而 `charged = tokens > 0` 判它
    /// 「已充值可用」→ 界面绿着、引导横幅不出现 —— 报告是对的，世界是坏的。
    ///
    /// 这一位不替代 `charged`（他确实充过值，说他没充是撒谎），只是把
    /// 「快撞门槛了」显式说出来。
    pub low_balance: bool,
    /// 钱包 id（客服排障用；老路径下为空）
    #[serde(default)]
    pub wallet_id: String,
    /// 🔴 这台机器迁移过，但本地配置丢了 —— 老余额**技术上找不回来**了。
    /// GUI 必须据此提示「凭充值订单号联系客服」，而不是默默给个 ¥0 的新钱包
    /// 让客户以为钱蒸发了。丢状态不可怕，丢了还装作没丢才可怕。
    #[serde(default)]
    pub legacy_balance_unrecoverable: bool,
}

fn uking_home() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".uking")
}

fn cache_path() -> PathBuf {
    uking_home().join("device.json")
}

/// 取设备 Key（缓存优先），并顺手查一次余额。
pub fn get_device_key() -> Result<DeviceKey, String> {
    // 先把身份收敛好（绑定 / 迁移 / 把上次没走完的收尾）。best-effort，失败不阻塞。
    ensure_activated_best_effort();

    let st = load_state();
    let key = current_key()?;
    let mut balance = query_balance(&key).ok();
    // charged = 真有正余额（>0）。注意不能用 balance.is_some()：欠费/超用账户余额查询
    // 也成功（返回 tokens<=0），那种不算「已充值可用」，否则引导/状态会误判已就绪。
    let mut charged = balance.as_ref().map(|b| b.tokens > 0).unwrap_or(false);
    // 自愈：本地标了已就绪但服务端这把 token 实际不存在 → 余额永远 0、调模型直接 401
    // （Invalid token）。成因：① 老客户端在服务端加 mint 步骤（2026-06-13）之前就把
    // activated 写死了，从没真正 mint 过；② token 被清过。charged=false 时**强制再收敛
    // 一次**（无视本地标记）把这些机器救回来。沙箱下自身跳过，不污染 selfcheck。
    if !charged {
        force_reconcile();
        let k = current_key()?;
        balance = query_balance(&k).ok();
        charged = balance.as_ref().map(|b| b.tokens > 0).unwrap_or(false);
    }
    let key = current_key()?;
    // 门槛取 ¥0.5：实测挡住客户的那次预扣是 ¥0.358（Claude Code + 默认模型）。
    // 留 40% 余量是因为**预扣额随上下文和模型价浮动**，钉死在 0.358 会让「刚好过线
    // 又当场被拒」的人拿不到提醒 —— 提醒早一点只是多看一眼，晚一点就是发不出去还不知道为什么。
    const LOW_BALANCE_CNY: f64 = 0.5;
    let low_balance = balance
        .as_ref()
        .map(|b| b.tokens > 0 && b.cny < LOW_BALANCE_CNY)
        .unwrap_or(false);
    Ok(DeviceKey {
        // 充值页 = 国内可达的 u-claw.org.cn/recharge（带真实充值表单，读 key 参数）。
        // ⚠️ 不能用 api.u-claw.org / cloud.u-claw.org —— 这两个子域指向新加坡
        // 这两个子域指向新加坡服务器，国内裸网 TLS SNI 握手被 GFW reset（客户机 pc-***
        // 2026-06-17 实测：TCP 443 通但 Invoke-WebRequest「连接被关闭」）。
        // 只有 u-claw.org.cn/recharge 国内裸网 200 且有充值表单（实测确认）。
        recharge_url: format!("https://u-claw.org.cn/recharge?key={key}"),
        key,
        balance,
        charged,
        low_balance,
        wallet_id: st.wallet_id,
        legacy_balance_unrecoverable: st.legacy_unrecoverable,
    })
}

/// 拿到设备内置 Key（不查网络）—— 给装完工具后写配置用。
pub fn device_key_offline() -> Result<String, String> {
    current_key()
}

/// 填入一把已有的访问密钥（换电脑 / 多个副本共用 / 老手用网站生成的那把）。
///
/// # 为什么这件事这么简单
///
/// 它**不需要任何服务端调用**，除了验一次这把 key 真的能用。因为凭证的本质就是
/// 「一张充值卡」：谁拿着谁能花，服务端认的是卡本身，不关心它躺在哪台机器上。
/// 所以「把 A 电脑的 key 抄到 B 电脑」= 把字符串写进 device.json，没有第二步。
///
/// 这也是为什么我们**不做恢复码**：key 自己就是那张要备份的东西，再包一层
/// 「用来换 key 的码」只是多一个可丢的东西。
///
/// # 三个刻意的选择
///
/// - **必须先验通再落盘**。填错一个字符就静默写进去的话，客户会得到一台
///   「看起来配好了、一发消息就报错」的机器，而错误信息指向的是模型不是 key。
/// - **不动服务端的旧 key**。首启自动发的那把（0 余额）会留在服务端当孤儿 ——
///   0 额度的 token 没有成本，为它加一个「作废我自己」的端点是白增攻击面。
/// - **落成 `random`**，于是下次启动 `reconcile_identity` 直接跳过：不会拿别人机器的
///   key 去跑迁移，也不会把它当成本机指纹 key 处理。
pub fn adopt_device_key(key: &str) -> Result<String, String> {
    let _guard = wallet_operation_guard();
    let key = key.trim();
    if key.is_empty() {
        return Err("请先填入密钥".into());
    }
    // 只做最松的形状检查 —— 真正的判据是下面那次余额查询。
    // 卡格式会误伤：老 key 有 `sk-uc-` / `sk-xp-` 等历史前缀，网站生成的长度也未必一样。
    if !key.starts_with("sk-") || key.len() < 8 {
        return Err("这不像一把虾盘云密钥（应以 sk- 开头）".into());
    }
    if key.contains(char::is_whitespace) {
        return Err("密钥里混进了空格或换行，请重新复制".into());
    }

    // 唯一的判据：服务端认不认它。认了才落盘。
    let balance = query_balance(key)
        .map_err(|e| format!("这把密钥用不了，没有保存：{e}"))?;

    let mut st = load_state();
    st.key = key.to_string();
    st.kind = KIND_RANDOM.into();
    st.wallet_id = String::new(); // 别人的钱包 id 我们不知道，也不需要知道
    st.pending_key = None;
    st.pending_kind = String::new();
    st.pending_from = String::new();
    st.local_reset = false;
    // 换了一把能用的 key，之前那句「老余额找不回」就不该再挂着了。
    st.legacy_unrecoverable = false;
    save_state_checked(&st)?;

    // 导入和轮换一样都会改变本机当前 Key，必须同步真实消费者，不能只改 device.json。
    apply_wallet_to_consumers(Some(key))?;

    Ok(format!("已启用这把密钥，余额 {}", balance.text))
}

/// 用户点「更新访问密钥」。两步：mint → 验通 → commit。
///
/// **不是 best-effort**：用户是主动点的，失败必须原样说出来。静默失败会让他以为
/// 换过了、其实旧 key 还在到处能用 —— 「以为自己安全」比「知道自己不安全」更糟。
pub fn rotate_device_key() -> Result<String, String> {
    let _guard = wallet_operation_guard();
    if std::env::var("UKING_TEST_HOME").is_ok() {
        return Err("沙箱环境不执行密钥轮换".into());
    }
    let st = load_state();
    if st.kind != KIND_RANDOM {
        return Err("这台设备还没升级到新的密钥体系，请先联网启动一次".into());
    }
    let current = st.key.clone();
    if current.is_empty() {
        return Err("本机没有可用的访问密钥".into());
    }

    // 有没有上次没走完的？有就直接续，别再 mint 一把（会在服务端堆没人用的 token）。
    let issued = match st.pending_key.clone() {
        Some(p) if !p.is_empty() => p,
        _ => {
            let issue = providers::device_rotate(&current)?;
            save_pending(&issue.api_key, PENDING_ROTATE, &current, &issue.wallet_id)?;
            issue.api_key
        }
    };

    // 验证新 key 真的能用 —— 走**只读**的余额查询，不消耗额度。
    // 不验就 commit 的话，一旦新 key 有问题，旧的已经被吊销 = 客户当场断线。
    query_balance(&issued).map_err(|e| format!("新密钥验证失败，已保留旧密钥：{e}"))?;

    let moved = providers::device_rotate_commit(&current, &issued)?;
    promote_key(&issued)?;
    Ok(format!("已更新访问密钥，余额 {moved} token 已保留"))
}

/// 只移除本机钱包引用；服务端钱包、原 Key 和余额不删除。
pub fn reset_local_device_wallet() -> Result<String, String> {
    let _guard = wallet_operation_guard();
    let st = load_state();

    if let Some(pending) = st.pending_key.as_deref().filter(|key| !key.is_empty()) {
        match classify_pending(&st) {
            PendingPolicy::SettleKnown => {
                finish_pending(&st, pending);
                let settled = load_state();
                if settled.pending_key.as_deref().is_some_and(|key| !key.is_empty()) {
                    return Err("设备钱包还有一笔未完成的密钥操作，已保留本机钱包；请联网后重试".into());
                }
                // 收尾后当前 Key 可能已经变了，第一次确认与备份失效，必须重新展示再确认。
                return Err("设备钱包刚完成一笔未决操作，当前 Key 已刷新；请确认备份新 Key 后再次删除".into());
            }
            PendingPolicy::RejectUnknown => return Err("检测到无法识别的设备钱包未决操作，已保留全部本机状态；请升级或联系支持".into()),
            PendingPolicy::None => unreachable!("pending key 已在外层确认存在"),
        }
    }

    // 先清真实消费者。任一清理失败都保留 device.json，避免半删除状态。
    apply_wallet_to_consumers(None)?;
    let reset = DeviceState {
        kind: KIND_RANDOM.into(),
        local_reset: true,
        ..DeviceState::default()
    };
    if let Err(save_error) = save_state_checked(&reset) {
        // 消费者已经先清理；钱包状态落盘失败时尽量把旧 Key 写回，避免半删除。
        let rollback = apply_wallet_to_consumers(Some(&st.key));
        return Err(match rollback {
            Ok(()) => format!("移除本机钱包失败，已恢复原设备钱包配置：{save_error}"),
            Err(rollback_error) => format!(
                "移除本机钱包失败，且恢复原配置也失败：{save_error}；{rollback_error}"
            ),
        });
    }
    Ok("已从本机移除设备钱包；服务端钱包、原 Key 和余额均未删除".into())
}

#[derive(Debug, PartialEq, Eq)]
enum PendingPolicy {
    None,
    SettleKnown,
    RejectUnknown,
}

fn classify_pending(st: &DeviceState) -> PendingPolicy {
    if st.pending_key.as_deref().is_none_or(str::is_empty) {
        PendingPolicy::None
    } else if matches!(st.pending_kind.as_str(), PENDING_ROTATE | PENDING_MIGRATE) {
        PendingPolicy::SettleKnown
    } else {
        PendingPolicy::RejectUnknown
    }
}

/// 仅读缓存的 Key 前 12 位（bug 上报去重用，不查网络、不含完整 Key）。
pub fn get_device_key_cached_prefix() -> String {
    std::fs::read_to_string(cache_path())
        .ok()
        .and_then(|s| {
            serde_json::from_str::<serde_json::Value>(&s)
                .ok()
                .and_then(|v| v.get("key").and_then(|k| k.as_str()).map(|k| k.chars().take(12).collect()))
                .or_else(|| json_string_field(&s, "key").map(|k| k.chars().take(12).collect()))
        })
        .unwrap_or_else(|| "unknown".into())
}

// ---------------------------------------------------------------------------
// 本地状态（~/.uking/device.json）
// ---------------------------------------------------------------------------

/// device.json 的解析结果。字段全部可缺 —— 老版本写下的文件只有 `key` / `machine_guid`
/// / `activated`，读到那种就是「还没升级的老客户」。
#[derive(Debug, Default, Clone)]
struct DeviceState {
    key: String,
    /// `random`（服务端签发）/ `fingerprint`（老的硬件派生）。缺省当 fingerprint 看。
    kind: String,
    wallet_id: String,
    pending_key: Option<String>,
    pending_kind: String,
    /// 轮换/迁移的源 key。收尾 commit 要用它，不能靠「当前 key」推 ——
    /// 崩在 promote 之后、commit 之前时，当前 key 已经是新的了。
    pending_from: String,
    legacy_unrecoverable: bool,
    /// 用户明确移除过本机钱包：下一次联网直接 bind 新钱包，不再认领旧指纹钱包。
    local_reset: bool,
}

fn load_state() -> DeviceState {
    let raw = match std::fs::read_to_string(cache_path()) {
        Ok(s) => s,
        Err(_) => return DeviceState::default(),
    };
    let v: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        // 文件被写坏过（断电截断等）。退到宽松的手抠字段 —— 至少把 key 捞出来，
        // 比整个当成「新机器」再建一个空钱包强。
        Err(_) => {
            return DeviceState {
                key: json_string_field(&raw, "key").unwrap_or_default(),
                kind: json_string_field(&raw, "keyKind").unwrap_or_default(),
                ..Default::default()
            }
        }
    };
    let s = |k: &str| {
        v.get(k)
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .to_string()
    };
    let pending = s("pendingKey");
    DeviceState {
        key: s("key"),
        kind: {
            let k = s("keyKind");
            if k.is_empty() {
                KIND_FINGERPRINT.into()
            } else {
                k
            }
        },
        wallet_id: s("walletId"),
        pending_key: (!pending.is_empty()).then_some(pending),
        pending_kind: s("pendingKind"),
        pending_from: s("pendingFrom"),
        legacy_unrecoverable: v
            .get("legacyUnrecoverable")
            .and_then(|x| x.as_bool())
            .unwrap_or(false),
        local_reset: v.get("localReset").and_then(|x| x.as_bool()).unwrap_or(false),
    }
}

/// 合并写回 device.json。
///
/// **先写临时文件再 rename**：device.json 里装的是客户唯一能花钱的凭证，
/// 写到一半断电 = 客户的钱包凭证没了。rename 在同一分区上是原子的。
fn save_state(st: &DeviceState) {
    let _ = save_state_checked(st);
}

fn save_state_checked(st: &DeviceState) -> Result<(), String> {
    std::fs::create_dir_all(uking_home()).map_err(|e| format!("创建设备钱包目录失败: {e}"))?;
    let body = serde_json::json!({
        "key": st.key,
        "keyKind": st.kind,
        "walletId": st.wallet_id,
        "pendingKey": st.pending_key.clone().unwrap_or_default(),
        "pendingKind": st.pending_kind,
        "pendingFrom": st.pending_from,
        "legacyUnrecoverable": st.legacy_unrecoverable,
        "localReset": st.local_reset,
        "note": "U-King 设备访问凭证。keyKind=random 时由服务端随机签发，可在界面上更新；\
                 删除本文件会导致余额无法自动找回，请凭充值订单号联系客服。",
    });
    let p = cache_path();
    let tmp = p.with_extension("json.tmp");
    let encoded = serde_json::to_string_pretty(&body).map_err(|e| format!("序列化设备钱包失败: {e}"))?;
    std::fs::write(&tmp, encoded).map_err(|e| format!("写设备钱包临时文件失败: {e}"))?;
    std::fs::rename(&tmp, &p).map_err(|e| format!("保存设备钱包失败: {e}"))
}

/// 当前该拿去用的凭证。
fn current_key() -> Result<String, String> {
    let st = load_state();
    if !st.key.is_empty() {
        return Ok(st.key);
    }
    if st.local_reset {
        return Err("本机设备钱包已移除，请联网生成新的设备钱包".into());
    }
    // 一把都没有：只能先给指纹 key（老路径下它本身就是凭证；新路径下它至少
    // 能让 migrate 认领到钱包）。绝不返回空 —— 上层拿空 key 去写配置会写出一堆废配置。
    fingerprint_key()
}

fn save_pending(new_key: &str, kind: &str, from: &str, wallet_id: &str) -> Result<(), String> {
    let mut st = load_state();
    st.pending_key = Some(new_key.to_string());
    st.pending_kind = kind.to_string();
    st.pending_from = from.to_string();
    if !wallet_id.is_empty() {
        st.wallet_id = wallet_id.to_string();
    }
    save_state_checked(&st)
}

/// 「本机当前 Key 换了」之后该干的事，由组合根 `lib.rs` 在启动时注入。
///
/// 模块自己不认识「AI 工具」这个概念（四铁律：模块只暴露纯函数，不反向依赖组合根），
/// 所以这里只留一个出口，写回/清理落点的活由 `lib.rs` 复用驱动动作核心完成，
/// 不在这儿再实现第二份。
type WalletConsumerHook = dyn Fn(Option<&str>) -> Result<(), String> + Send + Sync;
static WALLET_CONSUMER_HOOK: OnceLock<Box<WalletConsumerHook>> = OnceLock::new();

/// 注册「Key 换了」的收尾动作。只认第一次注册，重复调用无效（幂等）。
pub fn set_wallet_consumer_hook(f: Box<WalletConsumerHook>) {
    let _ = WALLET_CONSUMER_HOOK.set(f);
}

fn apply_wallet_to_consumers(key: Option<&str>) -> Result<(), String> {
    match WALLET_CONSUMER_HOOK.get() {
        Some(f) => f(key),
        None if key.is_none() => Err("设备钱包消费者尚未初始化，已保留本机钱包".into()),
        None => Ok(()),
    }
}

/// commit 成功之后：新凭证转正，pending 清空。
///
/// 🔴 **这里是「本机当前 Key 变了」的唯一汇合点** —— 用户点「更新访问密钥」
/// （`rotate_device_key`）和启动时收尾 pending（`finish_pending`）都落到这儿。
///
/// 2026-08-19 pc-***：服务端轮换成功、余额也搬过去了，但这个函数当时只写
/// `~/.uking/device.json` 就返回了，**新 Key 一个字都没写回 claude / codex /
/// clawx / dsh / hermes / pi 这 6 个落点**。于是客户机继续拿着已被吊销的旧 Key，
/// 每次调用 401；而 new-api 对作废 token 报的是通用的「Invalid token」，跟
/// 「余额烧光」一字不差 —— 排障第一反应必然跑偏。客户看到的是：等 188 秒，
/// 然后一句「密钥没对上，多半是驱动没配好」，而驱动配得好好的。
///
/// **换 Key 不是记账，是改机器状态。** 记完账不落地，等于没换。
fn promote_key(new_key: &str) -> Result<(), String> {
    let mut st = load_state();
    st.key = new_key.to_string();
    st.kind = KIND_RANDOM.into();
    st.pending_key = None;
    st.pending_kind = String::new();
    st.pending_from = String::new();
    st.local_reset = false;
    save_state_checked(&st)?;

    // 落盘成功才谈写回：device.json 都没写成，写工具配置只会让两边更不一致。
    apply_wallet_to_consumers(Some(new_key))
}

// ---------------------------------------------------------------------------
// 身份收敛
// ---------------------------------------------------------------------------

/// 首启收敛（best-effort，幂等，可重复调）。
///
/// **沙箱（`UKING_TEST_HOME`）整个跳过** —— 否则 selfcheck 会在服务端真建号。
/// 绝不阻塞、失败静默：失败下次启动 / 查余额时自动重试。
///
/// 调用点：① app 启动后台线程（`lib.rs` setup）；② `get_device_key`（查余额前）。
pub fn ensure_activated_best_effort() {
    reconcile_identity(false);
}

/// 无视「看起来已经好了」强制再收敛一次。给「本地标了已就绪但服务端 token 实际缺失」
/// 的机器自愈用（见 `get_device_key`）。
fn force_reconcile() {
    reconcile_identity(true);
}

fn reconcile_identity(force: bool) {
    let _guard = wallet_operation_guard();
    reconcile_identity_locked(force);
}

fn reconcile_identity_locked(force: bool) {
    if std::env::var("UKING_TEST_HOME").is_ok() {
        return; // 沙箱/selfcheck 不真实建号
    }
    let st = load_state();

    // 明确移除过本机钱包时直接建新钱包，绝不再按硬件指纹认领旧钱包。
    if st.local_reset {
        if let Ok(fp) = fingerprint_key() {
            bind_fresh_wallet(&hw_hint(&fp), false, false);
        }
        return;
    }

    // ① 有没有上次没走完的？先收尾 —— 半途的状态每多活一次启动，
    //    就多一次「客户端和服务端谁是对的」的分歧机会。
    if let Some(pending) = st.pending_key.clone() {
        finish_pending(&st, &pending);
        return;
    }

    // ② 已经是随机凭证 → 顺手修复真实消费者，再结束。这样 adopt/rotate 在
    // “钱包已落盘、消费者写回失败”之间崩溃时，下次启动能自动收敛。
    if st.kind == KIND_RANDOM && !st.key.is_empty() && !force {
        let _ = apply_wallet_to_consumers(Some(&st.key));
        return;
    }
    if st.kind == KIND_RANDOM && !st.key.is_empty() {
        let _ = apply_wallet_to_consumers(Some(&st.key));
        return;
    }

    // ③ 老客户 / 全新机器：拿指纹 key 去认领。
    let fp = match fingerprint_key() {
        Ok(k) => k,
        Err(_) => return,
    };
    let hw = hw_hint(&fp);
    match providers::device_migrate(&fp, &hw, platform_tag()) {
        Ok(MigrateOutcome::Staged(issue)) => {
            if save_pending(&issue.api_key, PENDING_MIGRATE, &fp, &issue.wallet_id).is_err() {
                return;
            }
            let st = load_state();
            finish_pending(&st, &issue.api_key);
        }
        Ok(MigrateOutcome::NotOurCustomer) => bind_fresh_wallet(&hw, false, true),
        Ok(MigrateOutcome::AlreadyMigrated) => {
            // 这台机器迁过了、本地配置没了。老余额技术上找不回来 —— 建个新钱包，
            // 但**把这件事标出来**让 GUI 去说，别默默给个 ¥0 让客户以为钱蒸发了。
            bind_fresh_wallet(&hw, true, true);
        }
        Ok(MigrateOutcome::NotDeployed) => legacy_activate_fallback(&fp),
        Err(e) if e == DEVICE_API_NOT_DEPLOYED => legacy_activate_fallback(&fp),
        Err(_) => {} // 网络问题，下次再来
    }
}

/// 把 pending 走完：验通新 key → commit → 转正。
fn finish_pending(st: &DeviceState, pending: &str) {
    // 验证走**只读**余额查询，不消耗额度。验不过就保留现状（旧 key 还能用），下次重试。
    if query_balance(pending).is_err() {
        // 🔴 「下次重试」有个前提：重试得有机会变好。2026-08-19 真机上不成立 ——
        // 服务端把 pending 按 0 额度签发，而 new-api 拒绝一切 `remain_quota <= 0`
        // 的 token（报的还是通用的「Invalid token」）。于是这里永远验不过、永远不
        // commit，而**外面看一切正常**：老 key 还能用、余额还在、客户一个字都不会报。
        //
        // 所以验不过时主动回服务端要一次（幂等：同一把 pending 原样返回，并顺手把
        // 额度补到可验证）。还不行就真的等下次 —— 那才是网络问题该有的处置。
        if !reissue_pending(st, pending) {
            return;
        }
    }
    let from = if st.pending_from.is_empty() {
        st.key.clone()
    } else {
        st.pending_from.clone()
    };
    let ok = match st.pending_kind.as_str() {
        PENDING_MIGRATE => providers::device_migrate_commit(&from, pending).is_ok(),
        PENDING_ROTATE => providers::device_rotate_commit(&from, pending).is_ok(),
        // pendingKind 缺失（老文件 / 写坏了）。不猜是哪一种 —— 猜错了会打到
        // 另一条链路上，而两条链路对「源 key 之后还能不能用」的处置是相反的。
        _ => false,
    };
    if ok {
        let _ = promote_key(pending);
    }
}

/// 让服务端把这把 pending 重新发一遍，然后再验一次。验通了返回 true。
///
/// 服务端两条链路都对「已有 pending」做幂等：**原样返回同一把 key**，不会再 mint
/// 一把（否则每次重试都在 new-api 里堆一个没人用的 token）。所以这里的重发是安全的，
/// 代价只有一次请求。
///
/// 拿回来的是**另一把** key 时不当场用：只落盘，交给下一次收敛。这一轮的 `pending`
/// 已经被上面读进来了，就地换掉等于让 commit 用一个跟验证对象不同的 key ——
/// 两条链路对「源 key 之后还能不能用」的处置相反，串了就是拿客户的余额赌。
fn reissue_pending(st: &DeviceState, pending: &str) -> bool {
    let fresh = match st.pending_kind.as_str() {
        PENDING_MIGRATE => {
            let fp = if st.pending_from.is_empty() {
                match fingerprint_key() {
                    Ok(k) => k,
                    Err(_) => return false,
                }
            } else {
                st.pending_from.clone()
            };
            let hw = hw_hint(&fp);
            match providers::device_migrate(&fp, &hw, platform_tag()) {
                Ok(MigrateOutcome::Staged(issue)) => issue,
                // 其余结果（已迁过 / 不认识 / 没部署）都不该在「本地有 pending」时出现。
                // 不猜，留着现状等下一次收敛按正常分支处理。
                _ => return false,
            }
        }
        PENDING_ROTATE => {
            let from = if st.pending_from.is_empty() {
                st.key.clone()
            } else {
                st.pending_from.clone()
            };
            match providers::device_rotate(&from) {
                Ok(issue) => issue,
                Err(_) => return false,
            }
        }
        _ => return false,
    };
    if fresh.api_key != pending {
        let _ = save_pending(&fresh.api_key, &st.pending_kind.clone(), &st.pending_from, &fresh.wallet_id);
        return false;
    }
    query_balance(pending).is_ok()
}

fn bind_fresh_wallet(hw_hint: &str, legacy_lost: bool, allow_legacy_fallback: bool) {
    match providers::device_bind(hw_hint, platform_tag(), channel_tag()) {
        Ok(issue) => {
            let mut st = load_state();
            st.key = issue.api_key;
            st.kind = KIND_RANDOM.into();
            st.wallet_id = issue.wallet_id;
            st.pending_key = None;
            st.pending_kind = String::new();
            st.pending_from = String::new();
            st.legacy_unrecoverable = legacy_lost;
            st.local_reset = false;
            if save_state_checked(&st).is_ok() {
                let _ = apply_wallet_to_consumers(Some(&st.key));
            }
        }
        Err(e) if e == DEVICE_API_NOT_DEPLOYED && allow_legacy_fallback => {
            if let Ok(fp) = fingerprint_key() {
                legacy_activate_fallback(&fp);
            }
        }
        Err(_) => {}
    }
}

/// 老路：指纹 key + `/recharge/activate`。
///
/// 只在服务端还没部署 `/device/*` 时走。**客户端可能比服务端先发出去** ——
/// 退不回去就等于新版一装上就没 key。等存量升完再删这段。
fn legacy_activate_fallback(fp: &str) {
    if providers::activate_key(fp).is_ok() {
        let mut st = load_state();
        st.key = fp.to_string();
        st.kind = KIND_FINGERPRINT.into();
        st.local_reset = false;
        save_state(&st);
    }
}

/// 给服务端的硬件线索。**送哈希不送原值** —— 原值（MachineGuid）一旦进了我们的
/// 日志和数据库，泄露一次就等于把所有老客户的指纹 key 一起泄露了（那 key 就是它算的）。
fn hw_hint(fingerprint_key: &str) -> String {
    fingerprint_key
        .strip_prefix("sk-")
        .unwrap_or(fingerprint_key)
        .chars()
        .take(16)
        .collect()
}

fn platform_tag() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

/// 分发渠道。留着给「U 盘/激活码渠道保留赠送、开源下载版不送」用 ——
/// 现在服务端 bonusCNY 全局为 0，这个值只进日志。
fn channel_tag() -> &'static str {
    if cfg!(feature = "usb-guard") {
        "usb"
    } else {
        "download"
    }
}

/// 硬件指纹派生的老 key。**它不再是凭证**，只用来认领老客户的钱包。
fn fingerprint_key() -> Result<String, String> {
    let guid = machine_guid()?;
    let hex = sha256_hex(&format!("uking|{guid}"))?;
    Ok(format!("sk-{hex}"))
}

/// 把 `activated:true` 合并写回 device.json（保留 key/machine_guid/note 等既有字段）。
fn json_string_field(raw: &str, field: &str) -> Option<String> {
    let needle = format!("\"{field}\"");
    let start = raw.find(&needle)? + needle.len();
    let rest = raw[start..].trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    let mut chars = rest.strip_prefix('"')?.chars();
    let mut out = String::new();
    let mut escaped = false;
    while let Some(ch) = chars.next() {
        if escaped {
            out.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            return Some(out);
        } else {
            out.push(ch);
        }
    }
    None
}

/// 读 Windows MachineGuid（HKLM 读操作免管理员）。
///
/// 主路径用 `reg query`；reg.exe 缺失（精简系统/WinPE）或读不到时，退到由稳定环境
/// 标识拼出的兜底指纹，**保证永不返回 Err** —— 设备 Key 是产品命脉，宁可指纹略弱
/// 也不能因为一个外部命令缺失就整个拿不到 Key。兜底指纹同机也恒定（COMPUTERNAME +
/// USERNAME + 系统盘卷序列号都不随时间变）。
#[cfg(windows)]
fn read_reg_guid() -> Option<String> {
    // reg 用绝对路径：客户机 PATH 被改坏（System32 失联，pc-*** 实锤）时裸 reg
    // 解析失败会走兜底指纹 → 同一台机器 Key 漂移、余额身份丢失。
    let out = run_hidden(
        &crate::installer::system_tool("reg"),
        &["query", r"HKLM\SOFTWARE\Microsoft\Cryptography", "/v", "MachineGuid"],
    )
    .ok()?;
    out.lines()
        .find_map(|l| {
            let l = l.trim();
            l.starts_with("MachineGuid")
                .then(|| l.split_whitespace().last().unwrap_or("").to_string())
        })
        .filter(|g| !g.is_empty())
}

/// reg 缺失/读不到时的稳定环境标识兜底（同机恒定）。
#[cfg(windows)]
fn env_fallback_guid() -> String {
    let computer = std::env::var("COMPUTERNAME").unwrap_or_default();
    let user = std::env::var("USERNAME").unwrap_or_default();
    let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
    let fallback = format!("fallback|{computer}|{user}|{drive}");
    // 即使三个环境变量全空（极端受限账户），fallback 仍是稳定常量字符串 → Key 仍可生成。
    fallback
}

#[cfg(windows)]
fn machine_guid() -> Result<String, String> {
    if let Some(g) = read_reg_guid() {
        return Ok(g);
    }
    Ok(env_fallback_guid())
}

/// 带来源标注的机器指纹读取（`--selfcheck` 用：ioreg / reg / 兜底 各来自哪条路）。
/// 与 `machine_guid()` 共用同一批底层读取函数，杜绝两处判断漂移（宪法第 8 条）。
#[cfg(windows)]
pub fn machine_guid_probe() -> Result<(String, &'static str), String> {
    if let Some(g) = read_reg_guid() {
        Ok((g, "reg"))
    } else {
        Ok((env_fallback_guid(), "reg-fallback"))
    }
}

/// macOS：IOPlatformUUID（同一台 Mac 恒定，与虾盘云硬件绑定语义一致）。
#[cfg(target_os = "macos")]
fn machine_guid() -> Result<String, String> {
    machine_guid_probe().map(|(g, _)| g)
}

/// 带来源标注的机器指纹读取（`--selfcheck` 用）。ioreg 读不到时如实报 Err，
/// 与 `machine_guid()` 行为一致 —— 同一事实只有一份。
#[cfg(target_os = "macos")]
pub fn machine_guid_probe() -> Result<(String, &'static str), String> {
    let out = run_hidden(
        "sh",
        &[
            "-c",
            r#"ioreg -rd1 -c IOPlatformExpertDevice | awk -F'"' '/IOPlatformUUID/{print $4}'"#,
        ],
    )?;
    let g = out.trim().to_string();
    if g.is_empty() {
        Err("读取 IOPlatformUUID 失败".into())
    } else {
        Ok((g, "ioreg"))
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
fn machine_guid() -> Result<String, String> {
    machine_guid_probe().map(|(g, _)| g)
}

/// 带来源标注的机器指纹读取（`--selfcheck` 用）：Linux 走 machine-id。
#[cfg(not(any(windows, target_os = "macos")))]
pub fn machine_guid_probe() -> Result<(String, &'static str), String> {
    let g = std::fs::read_to_string("/etc/machine-id")
        .map(|s| s.trim().to_string())
        .map_err(|e| format!("读取 machine-id 失败: {e}"))?;
    Ok((g, "machine-id"))
}

/// 算 SHA256（**纯 Rust std 实现，零外部依赖**）。
///
/// 历史上 Windows 走 PowerShell、Unix 走 shasum —— 但客户机 PowerShell/.NET 坏掉
/// （`System.Net.ServicePointManager` 类初始化异常，pc bug #21/#22）时，设备 Key
/// 生成会整体失败 → 拿不到 Key → 配不了虾盘云。改成进程内纯算法实现，任何破机器
/// （PowerShell 被禁/损坏/老版本、shasum 缺失）都能算出来，绝不依赖外部 shell。
fn sha256_hex(input: &str) -> Result<String, String> {
    let digest = sha256(input.as_bytes());
    let mut hex = String::with_capacity(64);
    for b in digest {
        hex.push(char::from_digit((b >> 4) as u32, 16).unwrap());
        hex.push(char::from_digit((b & 0xf) as u32, 16).unwrap());
    }
    Ok(hex)
}

/// 对任意字节算 SHA-256 小写十六进制。供下载完整性校验等跨模块复用（纯 std，无 crate）。
pub(crate) fn sha256_hex_bytes(data: &[u8]) -> String {
    let digest = sha256(data);
    let mut hex = String::with_capacity(64);
    for b in digest {
        hex.push(char::from_digit((b >> 4) as u32, 16).unwrap());
        hex.push(char::from_digit((b & 0xf) as u32, 16).unwrap());
    }
    hex
}

/// 标准 SHA-256（FIPS 180-4），返回 32 字节摘要。纯 std，无 crate。
fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    // padding
    let mut msg = data.to_vec();
    let bit_len = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, word) in w.iter_mut().take(16).enumerate() {
            *word = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut out = [0u8; 32];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{classify_pending, sha256_hex, DeviceState, PendingPolicy, PENDING_MIGRATE, PENDING_ROTATE};

    #[test]
    fn sha256_known_vectors() {
        // 标准测试向量(FIPS 180-4)
        assert_eq!(
            sha256_hex("").unwrap(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex("abc").unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex("uking|test-guid").unwrap().len(),
            64
        );
    }

    #[test]
    fn reset_pending_policy_never_guesses_unknown_operations() {
        let mut state = DeviceState::default();
        assert_eq!(classify_pending(&state), PendingPolicy::None);

        state.pending_key = Some("sk-pending".into());
        state.pending_kind = PENDING_ROTATE.into();
        assert_eq!(classify_pending(&state), PendingPolicy::SettleKnown);

        state.pending_kind = PENDING_MIGRATE.into();
        assert_eq!(classify_pending(&state), PendingPolicy::SettleKnown);

        state.pending_kind = "future-server-operation".into();
        assert_eq!(classify_pending(&state), PendingPolicy::RejectUnknown);
    }
}

fn run_hidden(program: &str, args: &[&str]) -> Result<String, String> {
    #[cfg(windows)]
    let mut c = {
        use std::os::windows::process::CommandExt;
        let mut c = std::process::Command::new(program);
        c.creation_flags(0x0800_0000);
        c
    };
    #[cfg(not(windows))]
    let mut c = std::process::Command::new(program);

    let out = c
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("启动 {program} 失败: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    } else {
        Err(format!(
            "{program} 退出码 {}：{}",
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stderr)
        ))
    }
}
