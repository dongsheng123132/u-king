//! 硬件体检 —— 给「本地大模型」板块判断「这台机器能跑多大的模型」。
//!
//! ## 为什么要它
//!
//! 本地大模型成败的关键不是「能不能装」，而是**管理预期**：国内小白笔记本多是
//! 8–16G 内存、无独显，只能跑 3B–7B 小模型，体验比虾盘云满血 DeepSeek-V4 差一截。
//! 装之前先把「你这配置只能跑 X，效果如何」直白说清楚，避免「装完又慢又笨」砸口碑。
//!
//! ## 实现
//!
//! 纯 std + 子进程，不引第三方 crate（守 4.4MB 体积红线）。所有子进程 `CREATE_NO_WINDOW`
//! 防闪黑窗（与 installer.rs 同口径）。
//! - 内存：Windows `Get-CimInstance Win32_ComputerSystem`（退 wmic）；Mac `sysctl hw.memsize`
//! - 显卡/显存：优先 `nvidia-smi`（独显最准），退 Windows `Win32_VideoController` / Mac
//!   `system_profiler SPDisplaysDataType`
//! - CPU：`std::thread::available_parallelism`

use serde::Serialize;
use std::process::{Command, Stdio};

// ============================================================
// 数据模型
// ============================================================

#[derive(Debug, Clone, Serialize)]
pub struct HardwareInfo {
    /// 物理内存（MB）。0 = 探测失败
    pub ram_total_mb: u64,
    /// 显卡名（探到才有）
    pub gpu_name: Option<String>,
    /// 显存（MB）。独显走 nvidia-smi 最准；核显/探不到为 None
    pub gpu_vram_mb: Option<u64>,
    /// 厂商归一：nvidia / amd / intel / apple / unknown
    pub gpu_vendor: String,
    /// 是否有「能加速推理」的硬件（独显 / Apple Silicon 统一内存）
    pub gpu_accelerated: bool,
    /// 逻辑核心数
    pub cpu_cores: usize,
    /// 平台：windows / macos / 其它
    pub os: String,
    /// 推荐档位（直白预期文案在 note 里）
    pub recommend: ModelRecommend,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelRecommend {
    /// 档位：tiny / small / medium / large
    pub tier: String,
    /// Ollama 模型 id（`ollama run <model>`）
    pub model: String,
    /// 友好名
    pub model_label: String,
    /// 模型下载大小估计（GB），让用户对磁盘/下载有预期
    pub size_gb: f64,
    /// 直白预期管理文案（这台机器跑这个会是什么体验）
    pub note: String,
    /// 是否能跑出「像样」的体验（false = 能跑但慢/弱，UI 要提醒用虾盘云）
    pub can_run_decent: bool,
    /// 其它可选模型（同档位备选 + 进阶档位，UI 给「换一个」）
    pub alternatives: Vec<AltModel>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AltModel {
    pub model: String,
    pub label: String,
    pub size_gb: f64,
    /// 一句话定位
    pub note: String,
}

// ============================================================
// 探测
// ============================================================

/// 跑命令收 stdout（失败/空返回 None）。CREATE_NO_WINDOW 防黑窗。
fn cmd_out(program: &str, args: &[&str]) -> Option<String> {
    let mut c = Command::new(program);
    c.args(args).stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    let out = c.output().ok()?;
    // nvidia-smi 等工具有时非零退出仍打有用 stdout，所以只看 stdout 非空
    let s = String::from_utf8_lossy(&out.stdout).to_string();
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(s)
    }
}

#[cfg(windows)]
fn powershell(script: &str) -> Option<String> {
    cmd_out("powershell", &["-NoProfile", "-NonInteractive", "-Command", script])
}

/// 物理内存（MB）。
fn ram_total_mb() -> u64 {
    #[cfg(windows)]
    {
        // CIM 首选；老机器 / WMI 坏了退 wmic
        if let Some(s) = powershell("(Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory") {
            if let Ok(b) = s.trim().parse::<u64>() {
                return b / 1024 / 1024;
            }
        }
        if let Some(s) = cmd_out("wmic", &["ComputerSystem", "get", "TotalPhysicalMemory"]) {
            for line in s.lines() {
                if let Ok(b) = line.trim().parse::<u64>() {
                    return b / 1024 / 1024;
                }
            }
        }
        0
    }
    #[cfg(not(windows))]
    {
        if let Some(s) = cmd_out("sysctl", &["-n", "hw.memsize"]) {
            if let Ok(b) = s.trim().parse::<u64>() {
                return b / 1024 / 1024;
            }
        }
        0
    }
}

/// 厂商归一。
fn vendor_of(name: &str) -> &'static str {
    let n = name.to_ascii_lowercase();
    if n.contains("nvidia") || n.contains("geforce") || n.contains("rtx") || n.contains("gtx") || n.contains("quadro") || n.contains("tesla") {
        "nvidia"
    } else if n.contains("amd") || n.contains("radeon") || n.contains("ryzen") {
        "amd"
    } else if n.contains("apple") || n.contains(" m1") || n.contains(" m2") || n.contains(" m3") || n.contains(" m4") {
        "apple"
    } else if n.contains("intel") {
        "intel"
    } else {
        "unknown"
    }
}

/// nvidia-smi 查独显名+显存 —— 探 PATH 与常见绝对路径（很多消费机 nvidia-smi 不进 PATH）。
/// 输出形如：`NVIDIA GeForce RTX 4060, 8188`
fn nvidia_smi_query() -> Option<(String, Option<u64>)> {
    let mut progs: Vec<&str> = vec!["nvidia-smi"];
    #[cfg(windows)]
    {
        progs.push(r"C:\Windows\System32\nvidia-smi.exe");
        progs.push(r"C:\Program Files\NVIDIA Corporation\NVSMI\nvidia-smi.exe");
    }
    for prog in progs {
        if let Some(s) = cmd_out(
            prog,
            &["--query-gpu=name,memory.total", "--format=csv,noheader,nounits"],
        ) {
            if let Some(line) = s.lines().next() {
                let mut parts = line.splitn(2, ',');
                let name = parts.next().unwrap_or("").trim().to_string();
                let vram = parts.next().and_then(|v| v.trim().parse::<u64>().ok());
                if !name.is_empty() {
                    return Some((name, vram));
                }
            }
        }
    }
    None
}

/// Windows 注册表读独显显存（64 位 `qwMemorySize`）—— 绕开 Win32 `AdapterRAM` 的 uint32 截断。
/// 多显卡取最大值（独显通常最大）。走系统内置 reg.exe，纯子进程不引 crate（守体积红线）。
#[cfg(windows)]
fn gpu_vram_from_registry() -> Option<u64> {
    let s = cmd_out(
        "reg",
        &[
            "query",
            r"HKLM\SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}",
            "/s",
            "/v",
            "HardwareInformation.qwMemorySize",
        ],
    )?;
    let mut best_mb: u64 = 0;
    for line in s.lines() {
        let l = line.trim();
        if !l.contains("qwMemorySize") {
            continue;
        }
        // 形如：`HardwareInformation.qwMemorySize    REG_QWORD    0x300000000`
        if let Some(tok) = l.split_whitespace().last() {
            let bytes = tok
                .strip_prefix("0x")
                .or_else(|| tok.strip_prefix("0X"))
                .and_then(|hex| u64::from_str_radix(hex, 16).ok())
                .or_else(|| tok.parse::<u64>().ok());
            if let Some(b) = bytes {
                let mb = b / 1024 / 1024;
                if mb > best_mb {
                    best_mb = mb;
                }
            }
        }
    }
    (best_mb > 0).then_some(best_mb)
}

/// 显卡名 + 显存（MB）。返回 (name, vram_mb)。
///
/// 健壮性（修「有独显却报无独显」的三个真实根因）：
/// - **枚举全部显卡再挑独显**：混合显卡笔记本（Intel 核显 + NVIDIA/AMD 独显）里
///   Win32_VideoController 常把核显排第一，旧代码 `Select -First 1` 会选到核显 → 误判无独显。
/// - **显存走注册表 `qwMemorySize`（64 位）**：Win32 `AdapterRAM` 是 uint32，6G/8G 独显会被
///   截断成 ~2–4G，旧代码据此判 `accelerated` 会把真独显误杀。
/// - **nvidia-smi 探绝对路径**：很多消费机 nvidia-smi 不在 PATH。
fn detect_gpu() -> (Option<String>, Option<u64>) {
    // ① nvidia-smi —— 独显最准（显存、型号一把抓）
    if let Some((name, vram)) = nvidia_smi_query() {
        return (Some(name), vram);
    }

    #[cfg(windows)]
    {
        // ② 枚举全部显卡，优先挑独显（nvidia/amd），核显 / Basic Display 兜底。
        if let Some(s) = powershell(
            "Get-CimInstance Win32_VideoController | \
             ForEach-Object { \"$($_.Name)|$($_.AdapterRAM)\" }",
        ) {
            let mut cands: Vec<(String, Option<u64>)> = Vec::new();
            for line in s.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let mut parts = line.splitn(2, '|');
                let name = parts.next().unwrap_or("").trim().to_string();
                if name.is_empty() {
                    continue;
                }
                let vram = parts
                    .next()
                    .and_then(|v| v.trim().parse::<u64>().ok())
                    .map(|b| b / 1024 / 1024)
                    .filter(|&mb| mb > 0);
                cands.push((name, vram));
            }
            // 挑：独显 > 非 Basic-Display 的任意卡 > 第一张
            let pick = cands
                .iter()
                .find(|(n, _)| matches!(vendor_of(n), "nvidia" | "amd"))
                .or_else(|| {
                    cands
                        .iter()
                        .find(|(n, _)| !n.to_ascii_lowercase().contains("basic display"))
                })
                .or_else(|| cands.first());
            if let Some((name, adapter_vram)) = pick {
                // 独显显存优先用注册表（准），退 AdapterRAM（可能被截断，宁缺毋误导）
                let vram = if matches!(vendor_of(name), "nvidia" | "amd") {
                    gpu_vram_from_registry().or(*adapter_vram)
                } else {
                    *adapter_vram
                };
                return (Some(name.clone()), vram);
            }
        }
        (None, None)
    }
    #[cfg(not(windows))]
    {
        // ② Mac：system_profiler 取「Chipset Model」（Apple Silicon 是统一内存，无独立显存）
        if let Some(s) = cmd_out("system_profiler", &["SPDisplaysDataType"]) {
            for line in s.lines() {
                let l = line.trim();
                if let Some(rest) = l.strip_prefix("Chipset Model:") {
                    let name = rest.trim().to_string();
                    if !name.is_empty() {
                        return (Some(name), None);
                    }
                }
            }
        }
        (None, None)
    }
}

/// 完整体检 + 推荐。
pub fn detect_hardware() -> HardwareInfo {
    let ram_total_mb = ram_total_mb();
    let (gpu_name, gpu_vram_mb) = detect_gpu();
    let cpu_cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let os = std::env::consts::OS.to_string();

    let vendor = gpu_name.as_deref().map(vendor_of).unwrap_or("unknown");
    // Mac（统一内存）即便没显卡名也按 apple 算（Finder 下 system_profiler 偶尔超时）
    let vendor = if vendor == "unknown" && os == "macos" {
        "apple"
    } else {
        vendor
    };

    // 「能加速推理」：独显（nvidia/amd 且有像样显存）或 Apple Silicon 统一内存
    let gpu_accelerated = match vendor {
        // 探到 nvidia 名字基本就是独显（含 MX/GTX/RTX/Quadro）——不再被截断的显存误杀
        "nvidia" => true,
        // AMD 核显（APU 集显）也叫 Radeon，靠(注册表准值)显存区分：独显通常 ≥4G
        "amd" => gpu_vram_mb.map(|v| v >= 4000).unwrap_or(false),
        "apple" => true,
        _ => false,
    };

    let recommend = recommend(ram_total_mb, gpu_vram_mb, vendor, gpu_accelerated);

    HardwareInfo {
        ram_total_mb,
        gpu_name,
        gpu_vram_mb,
        gpu_vendor: vendor.to_string(),
        gpu_accelerated,
        cpu_cores,
        os,
        recommend,
    }
}

// ============================================================
// 模型推荐（这台机器跑多大）
// ============================================================

fn alt(model: &str, label: &str, size_gb: f64, note: &str) -> AltModel {
    AltModel { model: model.into(), label: label.into(), size_gb, note: note.into() }
}

/// 全量可选模型（UI「换一个」用）。从小到大，覆盖各档位。
fn all_alternatives() -> Vec<AltModel> {
    vec![
        alt("qwen2.5:3b", "通义千问 2.5 · 3B", 2.0, "最小，低配也能跑，聊天问答尚可"),
        alt("qwen2.5:7b", "通义千问 2.5 · 7B", 4.7, "日常够用，中文强，推荐主力"),
        alt("qwen2.5:14b", "通义千问 2.5 · 14B", 9.0, "更聪明，需 16G+ 内存或 8G+ 显存"),
        alt("qwen2.5:32b", "通义千问 2.5 · 32B", 20.0, "接近云端小模型，需高配"),
        alt("deepseek-r1:7b", "DeepSeek-R1 · 7B", 4.7, "带推理链，擅长数理"),
        alt("llama3.2:3b", "Llama 3.2 · 3B", 2.0, "英文场景轻量之选"),
        alt("gemma2:9b", "Gemma 2 · 9B", 5.5, "谷歌出品，综合均衡"),
    ]
}

/// 据内存/显存/是否能加速，选一个「这台机器跑得动且体验最好」的档位。
fn recommend(ram_mb: u64, vram_mb: Option<u64>, vendor: &str, accelerated: bool) -> ModelRecommend {
    let ram_gb = ram_mb as f64 / 1024.0;
    let vram_gb = vram_mb.map(|m| m as f64 / 1024.0);

    // 推理「预算」：独显看显存，Apple/CPU 看内存。CPU-only 即便内存大也跑不快，单独处理。
    let (tier, model, label, size, note, decent) = if !accelerated {
        // —— 无独显，纯 CPU 推理：能跑但慢，封顶 7B，低内存退 3B ——
        if ram_gb < 8.0 {
            (
                "tiny",
                "qwen2.5:3b",
                "通义千问 2.5 · 3B",
                2.0,
                format!(
                    "你的电脑无独立显卡、内存约 {ram:.0}G，只能跑最小的 3B 模型，纯 CPU 运行较慢，\
                     适合简单问答。要流畅又聪明，建议直接用「虾盘云」满血云端模型。",
                    ram = ram_gb
                ),
                false,
            )
        } else {
            (
                "small",
                "qwen2.5:7b",
                "通义千问 2.5 · 7B",
                4.7,
                format!(
                    "你的电脑无独立显卡、内存约 {ram:.0}G，可以跑 7B 模型，但纯 CPU 运行速度一般\
                     （回答会比云端慢明显）。追求速度和效果建议用「虾盘云」。",
                    ram = ram_gb
                ),
                false,
            )
        }
    } else if vendor == "apple" {
        // —— Apple Silicon 统一内存，Metal 加速，体验好 ——
        if ram_gb >= 32.0 {
            ("large", "qwen2.5:32b", "通义千问 2.5 · 32B", 20.0,
             format!("Apple 芯片 + 约 {:.0}G 统一内存，可流畅跑到 32B，体验接近云端小模型。", ram_gb), true)
        } else if ram_gb >= 18.0 {
            ("medium", "qwen2.5:14b", "通义千问 2.5 · 14B", 9.0,
             format!("Apple 芯片 + 约 {:.0}G 内存，跑 14B 很顺，体验不错。", ram_gb), true)
        } else if ram_gb >= 12.0 {
            ("small", "qwen2.5:7b", "通义千问 2.5 · 7B", 4.7,
             format!("Apple 芯片 + 约 {:.0}G 内存，7B 流畅，日常够用。", ram_gb), true)
        } else {
            ("tiny", "qwen2.5:3b", "通义千问 2.5 · 3B", 2.0,
             format!("Apple 芯片但内存仅约 {:.0}G，建议跑 3B，更大可能吃紧。", ram_gb), true)
        }
    } else {
        // —— NVIDIA/AMD 独显，按显存定档（显存比内存更决定能跑多大）——
        let v = vram_gb.unwrap_or(0.0);
        if v >= 16.0 {
            ("large", "qwen2.5:32b", "通义千问 2.5 · 32B", 20.0,
             format!("独显约 {:.0}G 显存，可跑到 32B，体验接近云端小模型。", v), true)
        } else if v >= 10.0 {
            ("medium", "qwen2.5:14b", "通义千问 2.5 · 14B", 9.0,
             format!("独显约 {:.0}G 显存，跑 14B 很顺，体验不错。", v), true)
        } else if v >= 6.0 {
            ("small", "qwen2.5:7b", "通义千问 2.5 · 7B", 4.7,
             format!("独显约 {:.0}G 显存，7B 流畅，日常够用。", v), true)
        } else if v >= 4.0 {
            ("tiny", "qwen2.5:3b", "通义千问 2.5 · 3B", 2.0,
             format!("独显显存约 {:.0}G 偏小，建议跑 3B，7B 可能吃紧。", v), true)
        } else {
            // 探到独显但显存读不准 → 给个稳妥的 7B 并说明
            ("small", "qwen2.5:7b", "通义千问 2.5 · 7B", 4.7,
             "检测到独立显卡（显存未能精确读取），先按 7B 推荐，跑不动可在下方换更小的。".to_string(), true)
        }
    };

    ModelRecommend {
        tier: tier.into(),
        model: model.into(),
        model_label: label.into(),
        size_gb: size,
        note,
        can_run_decent: decent,
        // 全量备选给 UI「换一个」；当前推荐已在 model 字段，前端高亮即可
        alternatives: all_alternatives(),
    }
}
