//! U 盘护符 —— 「弱保护」：丢了原装 U 盘就跑不起来，专治「拷个 exe 就白嫖」的退货党。
//!
//! ## 为什么要它
//!
//! 绿色版 `U-King.exe` 现在双击即跑、计费绑机器指纹（不绑 U 盘），客户把 exe 拷到桌面照用，
//! 退了 U 盘也不影响 —— 退货率高的根。本模块给「U 盘口味」加一道**轻**校验：没原装 U 盘就不启动。
//!
//! ## 两个口味（同一套代码，编译期分叉）
//!
//! - **下载版**（官网 funnel）：`pnpm tauri build`，**不带** `usb-guard` feature → 护符整段被编译剔除，
//!   一个字节不影响，随便下随便跑（你的 token 中转生意零风险）。
//! - **U 盘版**：`pnpm tauri build --features usb-guard` → 带护符，启动先查原装 U 盘。
//!
//! ## 判定（弱、但够挡随手党）
//!
//! 启动放行条件（满足其一即可）：
//! 1. 本机「装到本地」过（正版 U 盘装的）—— `~/.uking/uking.lic` 与本机指纹匹配。官网主推的保留路径。
//! 2. 当前有**非系统盘**（≠ C:）挂着有效 `uking.key` —— 即原装 U 盘插着。
//!
//! 都不满足 → 弹友好原生提示框（不是崩溃），不启动主程序。
//!
//! **只认非系统盘**是关键：把 `exe`+`key` 一起拷到 C:\桌面 → 无效；拔了 U 盘 → 无效 →「丢盘不能跑」成立。
//! 而随手党只拷根目录的 `U-King.exe`（`uking.key` 藏在 `U-King\` 子目录里，他不会去拷）→ 直接被挡。
//!
//! 这是**弱保护**：拷整个 U 盘 / 用 devtools 的人照样破，但目标就是挡「一拷就发现能白嫖」的随手党，够用且轻。
//! 纯 std + 一点 Win32 FFI（MessageBoxW），不引第三方 crate。

use std::path::PathBuf;

/// 内嵌盐（弱保护，可被反编译提取 —— 无所谓，只用来让 `uking.key` 不能被 `echo` 随手造）。
const SALT: &str = "uking-usb-guard-2026-6f3a91";

/// 本口味是否带护符（编译期常量：带 `usb-guard` feature 才 true）。
pub fn is_enabled() -> bool {
    cfg!(feature = "usb-guard")
}

fn uking_home() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".uking")
}

/// 原装 U 盘密钥文件应有的内容（同一个盐 → 所有 U 盘同一个 key，烧录零额外工序）。
fn expected_usb_token() -> String {
    crate::device::sha256_hex_bytes(format!("uking-usb-genuine|{SALT}").as_bytes())
}

/// 本机「装到本地」授权令牌（绑设备指纹 Key —— 拷 ~/.uking 到别的机器会失配）。
fn local_token() -> String {
    let key = crate::device::device_key_offline().unwrap_or_default();
    crate::device::sha256_hex_bytes(format!("uking-local|{SALT}|{key}").as_bytes())
}

/// 本机是否已「装到本地」授权（正版 U 盘装的会写这个）。
fn machine_activated() -> bool {
    match std::fs::read_to_string(uking_home().join("uking.lic")) {
        Ok(s) => s.trim() == local_token(),
        Err(_) => false,
    }
}

/// 「装到本地」成功后调用：写本机授权，之后不插 U 盘也能跑。
/// 只在带护符口味下写（下载版调用即空转，不留垃圾文件）。
pub fn mark_local_activation() {
    if !is_enabled() {
        return;
    }
    let home = uking_home();
    let _ = std::fs::create_dir_all(&home);
    let _ = std::fs::write(home.join("uking.lic"), local_token());
}

/// 当前是否有原装 U 盘插着（非系统盘 + 有效 `uking.key`）。
#[cfg(windows)]
fn genuine_usb_present() -> bool {
    let sys = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
    let sys_letter = sys.chars().next().unwrap_or('C').to_ascii_uppercase();
    let want = expected_usb_token();
    for l in b'A'..=b'Z' {
        let letter = l as char;
        if letter == sys_letter {
            continue; // 系统盘（C:）上的 key 不认 —— 挡「拷到 C 盘白嫖」
        }
        let root = format!("{letter}:\\");
        if !std::path::Path::new(&root).exists() {
            continue;
        }
        // key 藏在 U-King\ 子目录（不显眼、随手党不会拷）；也兼容放根目录。
        for rel in ["U-King\\uking.key", "uking.key"] {
            let p = format!("{letter}:\\{rel}");
            if let Ok(s) = std::fs::read_to_string(&p) {
                if s.trim() == want {
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(not(windows))]
fn genuine_usb_present() -> bool {
    // Mac/Linux：扫 /Volumes（Mac）挂载点找 uking.key。U 盘护符主战场是 Windows，
    // 非 Windows 目前不带 usb-guard 编译，此实现仅兜底、绝不误伤。
    let want = expected_usb_token();
    for base in ["/Volumes", "/media", "/mnt", "/run/media"] {
        if let Ok(entries) = std::fs::read_dir(base) {
            for e in entries.flatten() {
                for rel in ["U-King/uking.key", "uking.key"] {
                    if let Ok(s) = std::fs::read_to_string(e.path().join(rel)) {
                        if s.trim() == want {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}

/// 放行判定：装到本地过 或 原装 U 盘插着。下载版（未启用）恒放行。
pub fn ok() -> bool {
    if !is_enabled() {
        return true;
    }
    // 沙箱/自检不校验（selfcheck / --*-test 不该被护符挡）。
    if std::env::var("UKING_TEST_HOME").is_ok() {
        return true;
    }
    machine_activated() || genuine_usb_present()
}

const GATE_TITLE: &str = "U-King · 未检测到原装 U 盘";
const GATE_BODY: &str = "\
你这份是「原装 U 盘」版本，但现在没检测到原装 U 盘。\n\
\n\
· 想继续用：插好随附的 U 盘，再点【重试】完成授权。\n\
· 其他问题，请联系微信 hecare888。\n\
\n\
点【取消】退出。";

/// 强制校验（启动早期调）：不通过就弹框，循环直到插上 U 盘点重试、或退出。
/// 只在带护符口味、Windows 下有实际拦截效果。
#[cfg(windows)]
pub fn enforce() {
    loop {
        if ok() {
            return;
        }
        if !message_box_retry(GATE_TITLE, GATE_BODY) {
            std::process::exit(0); // 取消 → 静默退出
        }
        // 重试 → 再查一遍（用户此时可能已插好 U 盘）
    }
}

#[cfg(not(windows))]
pub fn enforce() {
    // 非 Windows 不做拦截（该平台不带 usb-guard 编译）。
    let _ = (GATE_TITLE, GATE_BODY);
}

/// 原生 MessageBox（MB_RETRYCANCEL）。返回 true=用户点了「重试」，false=「取消」。
/// 直接 FFI user32，不引第三方 crate（对齐「体积优先」取舍）。
#[cfg(windows)]
fn message_box_retry(title: &str, body: &str) -> bool {
    #[link(name = "user32")]
    extern "system" {
        fn MessageBoxW(
            hwnd: *mut core::ffi::c_void,
            text: *const u16,
            caption: *const u16,
            utype: u32,
        ) -> i32;
    }
    let to_w = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let body_w = to_w(body);
    let title_w = to_w(title);
    // MB_RETRYCANCEL(0x5) | MB_ICONINFORMATION(0x40) | MB_SETFOREGROUND(0x10000) | MB_TOPMOST(0x40000)
    let ret = unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            body_w.as_ptr(),
            title_w.as_ptr(),
            0x5 | 0x40 | 0x0001_0000 | 0x0004_0000,
        )
    };
    ret == 4 // IDRETRY
}

/// 给一把 U 盘「盖章」（写入护符密钥）：烧录/补盘时用。
/// `root` = U 盘盘符或目录，如 `I:` / `I:\` / `/Volumes/UKING`。写入 `<root>/U-King/uking.key`。
pub fn arm_usb(root: &str) -> Result<String, String> {
    if root.trim().is_empty() {
        return Err("请指定 U 盘盘符/目录，例如：--arm-usb I:".into());
    }
    // Windows 上 `I:`（无反斜杠）指的是「I 盘当前目录」，会写歪 —— 补成 `I:\`。
    let root_norm = if root.ends_with(':') {
        format!("{root}\\")
    } else {
        root.to_string()
    };
    let dir = std::path::Path::new(&root_norm).join("U-King");
    std::fs::create_dir_all(&dir).map_err(|e| format!("建目录失败 {}: {e}", dir.display()))?;
    let p = dir.join("uking.key");
    std::fs::write(&p, expected_usb_token()).map_err(|e| format!("写密钥失败 {}: {e}", p.display()))?;
    Ok(p.display().to_string())
}

/// 打印护符密钥内容（发盘/排查时用，让人能手动放 `uking.key`）。
pub fn usb_token() -> String {
    expected_usb_token()
}

/// 无头诊断：给 `--guard-check` 用，也给售后在客户机上看「为什么被挡」。
/// 返回 JSON 字符串：本口味是否带护符 / 本机已装到本地授权 / 原装 U 盘在 / 最终放行。
/// 绕过沙箱短路，报**真实**子结果（`ok` 字段仍是含沙箱短路的最终判定）。
pub fn diagnose_json() -> String {
    let enabled = is_enabled();
    let activated = machine_activated();
    let usb = genuine_usb_present();
    format!(
        "{{\"enabled\":{enabled},\"machine_activated\":{activated},\"usb_present\":{usb},\"ok\":{}}}",
        ok()
    )
}
