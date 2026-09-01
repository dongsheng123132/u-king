//! 内置小程序 —— 随 exe 一起发货，首启自动装好。
//!
//! 为什么内置而不是联网下载：这几个加起来才 ~100KB，而「插上 U 盘就能用」是产品前提。
//! 客户机常年在裸网/代理坏掉的环境里（线上装机失败的 issue 大半是网络），
//! 让开箱可用的能力依赖一次下载，等于把最基础的体验押在最不可靠的一环上。
//!
//! 文件表是**显式**的：少写一个文件编译期就报错。用「递归打包整个目录」的写法看着省事，
//! 但目录里多一个 .data/ 或临时文件就会被一起发出去，少一个文件则要等客户点开才发现。
//!
//! 安装一律走 `miniapp::install_from_path` —— 那条路已经做了清单校验、路径穿越拦截、
//! staging→校验→原子换入。这里绝不自己写第二份解包逻辑。
//!
//! 删除本模块：去掉 lib.rs 里的 `mod bundled_apps;` 与那一次 `ensure_installed()` 调用即可。

use std::path::Path;

/// 一个内置小程序：`(相对路径, 文件内容)` 的显式清单。
struct Bundled {
    /// 与 uking-app.json 里的 app.id 一致，用来判断本机是不是已经有了
    id: &'static str,
    /// 与 uking-app.json 里的 app.version 一致，用来判断要不要升级
    version: &'static str,
    files: &'static [(&'static str, &'static [u8])],
}

const APPS: &[Bundled] = &[
    Bundled {
        id: "org.uking.app.idcard",
        version: "0.1.1",
        files: &[
            ("uking-app.json", include_bytes!("../apps/idcard/uking-app.json")),
            ("action-parity.json", include_bytes!("../apps/idcard/action-parity.json")),
            ("web/index.html", include_bytes!("../apps/idcard/web/index.html")),
            ("web/actions.mjs", include_bytes!("../apps/idcard/web/actions.mjs")),
        ],
    },
    Bundled {
        id: "org.uking.app.imagefix",
        // 0.1.1：修「界面读 r.image」——不提版本号，下面的不降级规则会让
        // 已装 0.1.0 的客户机直接 continue，修复永远到不了他手上。
        version: "0.1.2",
        files: &[
            ("uking-app.json", include_bytes!("../apps/imagefix/uking-app.json")),
            ("action-parity.json", include_bytes!("../apps/imagefix/action-parity.json")),
            ("icon.png", include_bytes!("../apps/imagefix/icon.png")),
            ("web/index.html", include_bytes!("../apps/imagefix/web/index.html")),
            ("web/actions.mjs", include_bytes!("../apps/imagefix/web/actions.mjs")),
        ],
    },
    Bundled {
        id: "org.example.app.resize",
        version: "0.1.0",
        files: &[
            ("uking-app.json", include_bytes!("../apps/resize/uking-app.json")),
            ("action-parity.json", include_bytes!("../apps/resize/action-parity.json")),
            ("web/index.html", include_bytes!("../apps/resize/web/index.html")),
            ("web/actions.mjs", include_bytes!("../apps/resize/web/actions.mjs")),
        ],
    },
];

/// `a < b`？只比较点分数字段，非数字段当 0。
fn version_lt(a: &str, b: &str) -> bool {
    let seg = |s: &str| -> Vec<u32> { s.split('.').map(|x| x.trim().parse().unwrap_or(0)).collect() };
    let (x, y) = (seg(a), seg(b));
    for i in 0..x.len().max(y.len()) {
        let (p, q) = (x.get(i).copied().unwrap_or(0), y.get(i).copied().unwrap_or(0));
        if p != q {
            return p < q;
        }
    }
    false
}

/// 把内置小程序装好（幂等）。已经装了同版本或更新的就跳过。
///
/// **不覆盖用户手上更新的版本**：客户可能自己装了作者发的新版，
/// 我们内置的这份是出厂垫底，不该把它按回旧版。
///
/// 失败只记日志不打断启动：内置小程序装不上是「少了几个可选工具」，
/// 不该让整个 U-King 起不来。
pub fn ensure_installed(on_log: &dyn Fn(&str)) {
    // 内置小程序首启自动装 —— **0.9.72 就是在这条路上全量翻车的**（客户机上三个小程序
    // 全打不开，而我们当时一行日志都没有，只能靠事后补 --miniapp-open 无头验证才定位）。
    // 这里每个 app 的「装/升级/跳过/失败」都留痕，下次同类问题第一时间看得到。
    let notify = on_log;
    let on_log = |m: &str| {
        crate::ulog::write("miniapp", m);
        notify(m);
    };
    // 用户主动删掉的，不再装回来。**必须在 install_one 之前判** —— 它内部走
    // `install_from_path`，而那条路会把墓碑清掉（显式重装才该清）。
    let removed = crate::miniapp::user_removed();

    for app in APPS {
        if removed.iter().any(|r| r == app.id) {
            on_log(&format!("[bundled] {} 用户删过，不自动装回（「补装内置」可恢复）", app.id));
            continue;
        }
        match crate::miniapp::get(app.id) {
            // 已装且不比内置的旧 → 什么都不做
            Ok(cur) if !version_lt(&cur.version, app.version) => continue,
            Ok(cur) => on_log(&format!(
                "[bundled] {} 本机 v{} 比内置 v{} 旧，升级",
                app.id, cur.version, app.version
            )),
            Err(_) => on_log(&format!("[bundled] {} 首次安装", app.id)),
        }
        if let Err(e) = install_one(app) {
            // 只记不抛：少一个内置小程序 ≠ U-King 起不来
            on_log(&format!("[bundled] {} 安装失败（跳过）：{e}", app.id));
        }
    }
}

fn install_one(app: &Bundled) -> Result<(), String> {
    // 先摊到临时目录，再走正规安装路径（校验/原子换入都在那边）
    let stage = std::env::temp_dir().join(format!("uking-bundled-{}", app.id));
    let _ = std::fs::remove_dir_all(&stage);
    for (rel, bytes) in app.files {
        let dst = stage.join(rel);
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("建目录失败: {e}"))?;
        }
        std::fs::write(&dst, bytes).map_err(|e| format!("写 {rel} 失败: {e}"))?;
    }
    let r = crate::miniapp::install_from_path(Path::new(&stage), "bundled").map(|_| ());
    let _ = std::fs::remove_dir_all(&stage);
    r
}

/// 内置了几个（给自检/排障用）。
pub fn count() -> usize {
    APPS.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 内置清单里的 id/version 必须和真实清单文件对得上 ——
    /// 对不上就会「判断说已是最新、其实装的是另一个」，是最难查的那类漂移。
    #[test]
    fn bundled_table_matches_manifests() {
        for app in APPS {
            let (rel, bytes) = app
                .files
                .iter()
                .find(|(r, _)| *r == "uking-app.json")
                .expect("每个内置小程序都必须带 uking-app.json");
            let v: serde_json::Value =
                serde_json::from_slice(bytes).unwrap_or_else(|e| panic!("{rel} 不是合法 JSON: {e}"));
            assert_eq!(v["app"]["id"].as_str().unwrap_or_default(), app.id, "内置表里的 id 与清单不一致");
            assert_eq!(
                v["app"]["version"].as_str().unwrap_or_default(),
                app.version,
                "内置表里的 version 与清单不一致（{}）",
                app.id
            );
        }
    }

    #[test]
    fn version_compare() {
        assert!(version_lt("0.1.0", "0.2.0"));
        assert!(version_lt("0.9.9", "0.10.0"));
        assert!(!version_lt("1.0.0", "1.0.0"));
        assert!(!version_lt("1.2.0", "1.1.9"));
    }
}
