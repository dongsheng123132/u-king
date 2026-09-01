//! 把「命令名」(`claude` / `codex`) 解析成一条**真正能 spawn 的 argv**。
//!
//! ## 为什么不能直接 spawn `claude.cmd`
//! npm 全局装的 CLI 在 Windows 上是一层 `.cmd` 批处理壳。Rust 从 1.77.2 起（BatBadBut /
//! CVE-2024-24576 的修补）在 spawn `.cmd`/`.bat` 时改走 cmd.exe 的引号规则，并且
//! **任何含 `\r` 或 `\n` 的参数一律拒绝**，直接返回
//! `InvalidInput: batch file arguments are invalid`。
//!
//! 我们每一轮恰好要传两个天然会带换行的参数：
//!   * `-p <用户提问>` —— 用户在输入框里敲了回车就有换行
//!   * `--append-system-prompt <persona>` —— `buildSystemPrompt` 是 `\n` 拼出来的，**必然**多行
//!
//! 于是「多行提问」失败、「召唤任意一位 AI 专家」100% 首次调用即失败，报错还长得像
//! 「启动失败 / 是否已安装」——把人往「没装好」这个错误方向带了整整一版。跟装没装无关。
//!
//! 实测（本机 rustc，`Command::new("x.cmd").arg(a)`）：中文 / `%` / `"` / `&` / `|` / `^` / `!`
//! 全部 OK，**只有 `\n` 和 `\r` 会被拒**。所以不是转义问题，靠「加引号」修不好。
//! 顺带一提，走 cmd.exe 还会把提示词里的 `%VAR%` 当环境变量展开 —— 静默改内容，更难查。
//!
//! ## 解法
//! 不走批处理壳：把壳里包着的那个 node 脚本挖出来，直接 `node <cli.js> …`。
//! CreateProcess 收的是 argv，没有第二次 shell 解析，换行 / 百分号 / 引号原样透传。
//!
//! 纯 std，不碰 AppHandle，只依赖 `installer` 的公共路径助手（依赖方向：agent → installer）。

use std::path::{Path, PathBuf};

use crate::installer::{portable_node_dir, search_paths};

/// 一条可以直接 spawn 的命令：`program` + 必须前置的参数（真实业务参数拼在后面）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launcher {
    /// 真实可执行文件（全路径优先）
    pub program: String,
    /// 前置参数（走 node 壳时是 `[cli.js]`，原生 exe 时为空）
    pub prefix: Vec<String>,
}

impl Launcher {
    /// 把前置参数和业务参数拼成完整 argv（给「看命令」展示用 —— 展示的必须是真跑的那条）。
    pub fn full_args(&self, args: &[String]) -> Vec<String> {
        let mut v = self.prefix.clone();
        v.extend_from_slice(args);
        v
    }

    fn bare(program: &str) -> Self {
        Launcher { program: program.to_string(), prefix: Vec::new() }
    }
}

/// node 可执行文件：便携版优先（与 installer / term 同口径），否则 PATH 上的 node。
fn node_exe() -> String {
    let name = if cfg!(windows) { "node.exe" } else { "node" };
    if let Some(d) = portable_node_dir() {
        let p = d.join(name);
        if p.exists() {
            return p.display().to_string();
        }
    }
    for dir in search_paths(portable_node_dir().as_deref()) {
        let p = dir.join(name);
        if p.exists() {
            return p.display().to_string();
        }
    }
    "node".to_string()
}

/// 把 npm/pnpm/yarn 生成的 `.cmd` 壳解开，找出它真正要起的东西。
///
/// 现实里**两种壳都存在**（本机 `%APPDATA%\npm\` 下一个一个，实测）：
/// ```text
/// claude.cmd → "%dp0%\node_modules\@anthropic-ai\claude-code\bin\claude.exe"   %*
/// codex.cmd  → "%_prog%"  "%dp0%\node_modules\@openai\codex\bin\codex.js" %*
/// ```
/// 所以只认 `.js` 会漏掉 Claude Code（它早就改发原生二进制了），这正是最要命的那一个。
///
/// 共同点：目标一定出现在**双引号里**。逐个候选试，取第一个在磁盘上真实存在的
/// —— 比「取最后一个」稳，pnpm 的 IF/ELSE 两个分支会写出两条路径、其中一条不存在。
///
/// 唯一的坑：npm 壳开头有一行 `IF EXIST "%dp0%\node.exe"` 是在探测「壳边上有没有自带
/// node」，那是**解释器**不是目标。认成目标就会变成「起了个空 node」——挂在那儿等 stdin，
/// 表现成「卡死」，比报错还难查。所以 stem 是 node 的 exe 一律跳过。
fn unwrap_shim(shim: &Path) -> Option<Launcher> {
    let text = std::fs::read(shim).ok()?;
    let text = String::from_utf8_lossy(&text);
    // `%~dp0` / `%dp0%` 都展开成壳所在目录（壳里紧跟着写 `\`，所以这里不带尾分隔符）
    let dir = shim.parent()?.display().to_string();
    let dir = dir.trim_end_matches(['\\', '/']).to_string();

    for raw in text.split('"') {
        let low = raw.to_ascii_lowercase();
        let is_exe = low.ends_with(".exe");
        let is_js = low.ends_with(".js") || low.ends_with(".mjs") || low.ends_with(".cjs");
        if !is_exe && !is_js {
            continue;
        }
        let expanded = raw
            .replace("%~dp0", &dir)
            .replace("%dp0%", &dir)
            .replace("%~dp0%", &dir);
        // 展开后还留着 `%…%` 说明有我们不认识的变量（如 `%_prog%`），别猜
        if expanded.contains('%') {
            continue;
        }
        let p = PathBuf::from(&expanded);
        if is_exe {
            // 解释器探测行，不是目标
            if p.file_stem().map(|s| s.eq_ignore_ascii_case("node")).unwrap_or(false) {
                continue;
            }
            if p.exists() {
                return Some(Launcher { program: p.display().to_string(), prefix: Vec::new() });
            }
        } else if p.exists() {
            return Some(Launcher { program: node_exe(), prefix: vec![p.display().to_string()] });
        }
    }
    None
}

/// 解析一个命令名。找不到就回落原名（让系统 PATH 再试一次），绝不返回 Err ——
/// 「起不来」的报错要留给 spawn 那一步说，这里假装失败只会多一层看不懂的错。
pub fn resolve(program: &str) -> Launcher {
    #[cfg(not(windows))]
    {
        for dir in search_paths(portable_node_dir().as_deref()) {
            let p = dir.join(program);
            if p.exists() {
                return Launcher { program: p.display().to_string(), prefix: Vec::new() };
            }
        }
        Launcher::bare(program)
    }

    #[cfg(windows)]
    {
        for dir in search_paths(portable_node_dir().as_deref()) {
            // ① 原生 exe 最好：CreateProcess 直收 argv，天然没有批处理那套毛病
            let exe = dir.join(format!("{program}.exe"));
            if exe.exists() {
                return Launcher { program: exe.display().to_string(), prefix: Vec::new() };
            }
            // ② .cmd/.bat 壳：解包成壳里包着的真身（原生 exe 或 `node <cli.js>`）
            for ext in [".cmd", ".bat"] {
                let shim = dir.join(format!("{program}{ext}"));
                if !shim.exists() {
                    continue;
                }
                if let Some(l) = unwrap_shim(&shim) {
                    return l;
                }
                // 解不开就照旧跑壳 —— 单行参数仍然能用，比直接放弃强。
                // （能走到这儿说明壳不是 node 包装器，或者脚本被删了。）
                return Launcher { program: shim.display().to_string(), prefix: Vec::new() };
            }
        }
        Launcher::bare(program)
    }
}

/// 一个 CLI 在**这台机器**上的启动体检结果。
///
/// 回答的是「它起不起得来」，不是「装没装」—— 这两件事今天刚被证明可以分家：
/// claude 明明装着、`--version` 也正常，但只要参数带换行就一个字都跑不出来。
/// 所以 `multiline_ok` 是独立的一项，不能从 `found` 推出来。
#[derive(Debug, Clone)]
pub struct Probe {
    pub program: String,
    /// 解析到的真实可执行文件（没找到时等于 program 本身）
    pub resolved: String,
    /// 走哪条路：exe=原生二进制 / node=node+脚本 / shim=没解开的批处理壳 / missing=没找到
    pub via: &'static str,
    pub found: bool,
    /// 参数里带换行还能不能 spawn（`false` = 多行提问和 AI 专家全废）
    pub multiline_ok: bool,
    pub error: Option<String>,
}

/// 真起一次进程做体检。**烧 0 token**：故意传一个不存在的 flag，
/// 我们只关心进程起没起来，不关心它退出码是几（起来了立刻 kill）。
///
/// 无头自检（`--agent-launch-test`）和 GUI「AI 优化大师 · 专项修复」共用这一份 ——
/// 同一个问题不许有两套判据，否则命令行说好、界面说坏，排障时先得排我们自己。
pub fn probe(program: &str) -> Probe {
    let l = resolve(program);
    let found = l.program != program;
    let low = l.program.to_ascii_lowercase();
    let via = if !found {
        "missing"
    } else if !l.prefix.is_empty() {
        "node"
    } else if low.ends_with(".cmd") || low.ends_with(".bat") {
        "shim"
    } else {
        "exe"
    };

    if !found {
        return Probe {
            program: program.into(),
            resolved: l.program,
            via,
            found: false,
            multiline_ok: false,
            error: Some("没在这台机器上找到它".into()),
        };
    }

    let mut c = std::process::Command::new(&l.program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    c.args(l.full_args(&["--uking-launch-probe".into(), "第一行\n第二行".into()]));
    c.stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null());

    let (multiline_ok, error) = match c.spawn() {
        Ok(mut ch) => {
            let _ = ch.kill();
            let _ = ch.wait();
            (true, None)
        }
        Err(e) => (false, Some(format!("{e}"))),
    };

    Probe { program: program.into(), resolved: l.program, via, found: true, multiline_ok, error }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ★ Claude Code 真身：`claude.cmd` 包的是**原生 exe**，不是 .js。
    /// 这条最关键 —— 只认 .js 的解包器会在这里悄悄回落到「照旧跑 .cmd」，
    /// bug 一个字都没修，测试却全绿。
    #[test]
    fn unwraps_shim_that_wraps_native_exe() {
        let root = std::env::temp_dir().join("uking-launcher-test-exe");
        let bin = root.join("node_modules").join("@anthropic-ai").join("claude-code").join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("claude.exe"), b"MZ").unwrap();
        let shim = root.join("claude.cmd");
        std::fs::write(
            &shim,
            "@ECHO off\r\nGOTO start\r\n:find_dp0\r\nSET dp0=%~dp0\r\nEXIT /b\r\n:start\r\n\
             \"%dp0%\\node_modules\\@anthropic-ai\\claude-code\\bin\\claude.exe\"   %*\r\n",
        )
        .unwrap();

        let l = unwrap_shim(&shim).expect("应当解出 claude.exe");
        assert!(l.program.to_ascii_lowercase().ends_with("claude.exe"), "解错了: {l:?}");
        assert!(l.prefix.is_empty(), "原生 exe 不该有前置脚本参数: {l:?}");
        assert!(Path::new(&l.program).exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// npm 的 node 壳（codex 真身就是这个形状）：解成 `node <cli.js>`。
    /// 同时钉死那个坑 —— 壳开头的 `IF EXIST "%dp0%\node.exe"` 是解释器探测，
    /// 认成目标就会起一个空 node 挂在那儿等 stdin（表现成卡死，比报错更难查）。
    #[test]
    fn unwraps_npm_node_shim_and_ignores_interpreter_probe() {
        let root = std::env::temp_dir().join("uking-launcher-test-npm");
        let pkg = root.join("node_modules").join("@openai").join("codex").join("bin");
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(pkg.join("codex.js"), "// fake").unwrap();
        // 陷阱：壳边上真的放一个 node.exe，逼解包器去区分「解释器」和「目标」
        std::fs::write(root.join("node.exe"), b"MZ").unwrap();
        let shim = root.join("codex.cmd");
        std::fs::write(
            &shim,
            "@ECHO off\r\nSET dp0=%~dp0\r\nIF EXIST \"%dp0%\\node.exe\" (\r\n  SET \"_prog=%dp0%\\node.exe\"\r\n)\r\n\
             \"%_prog%\"  \"%dp0%\\node_modules\\@openai\\codex\\bin\\codex.js\" %*\r\n",
        )
        .unwrap();

        let l = unwrap_shim(&shim).expect("应当解出 codex.js");
        assert!(
            !l.program.to_ascii_lowercase().ends_with("node.exe") || !l.prefix.is_empty(),
            "把解释器探测行当成目标了，会起一个空 node 卡死: {l:?}"
        );
        assert_eq!(l.prefix.len(), 1, "node 壳必须带上脚本: {l:?}");
        assert!(l.prefix[0].to_ascii_lowercase().ends_with("codex.js"), "取错脚本: {l:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// pnpm 壳有 IF/ELSE 两个分支、写的是 `%~dp0\..\..`，且**第一条路径可能不存在**。
    /// 必须取第一个真实存在的，不能盲取第一个或最后一个。
    #[test]
    fn unwraps_pnpm_style_shim_picking_existing_path() {
        let root = std::env::temp_dir().join("uking-launcher-test-pnpm");
        let bin = root.join(".bin");
        let pkg = root.join("node_modules").join("codex");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(pkg.join("index.js"), "// fake").unwrap();
        let shim = bin.join("codex.cmd");
        std::fs::write(
            &shim,
            // 第一条指向不存在的 missing.js，第二条才是真的
            "@IF EXIST \"%~dp0\\..\\node.exe\" (\r\n  \"%~dp0\\..\\missing.js\" %*\r\n) ELSE (\r\n\
               node  \"%~dp0\\..\\node_modules\\codex\\index.js\" %*\r\n)\r\n",
        )
        .unwrap();

        let l = unwrap_shim(&shim).expect("应当跳过不存在的那条，解出 index.js");
        assert!(l.prefix.first().map(|s| s.to_ascii_lowercase().ends_with("index.js")).unwrap_or(false), "取错了脚本: {l:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 不是包装器的壳（比如自己写的 .cmd）解不开 —— 要老实返回 None，别瞎猜。
    #[test]
    fn non_wrapper_shim_is_not_unwrapped() {
        let root = std::env::temp_dir().join("uking-launcher-test-plain");
        std::fs::create_dir_all(&root).unwrap();
        let shim = root.join("weird.cmd");
        std::fs::write(&shim, "@echo off\r\necho hi\r\n").unwrap();
        assert!(unwrap_shim(&shim).is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// full_args 必须把前置脚本拼在最前面 —— 「看命令」展示的和真跑的是同一条。
    #[test]
    fn full_args_prepends_prefix() {
        let l = Launcher { program: "node".into(), prefix: vec!["C:/cli.js".into()] };
        assert_eq!(l.full_args(&["-p".into(), "hi".into()]), vec!["C:/cli.js", "-p", "hi"]);
        let bare = Launcher::bare("claude");
        assert_eq!(bare.full_args(&["-p".into()]), vec!["-p"]);
    }

    /// 回归钉子：这个模块存在的唯一理由就是「参数里能带换行」。
    /// 直接 spawn .cmd 会被 std 拒（batch file arguments are invalid），解包后走 node 就不会。
    /// 这条用例不依赖机器上装没装 claude —— 自建一个壳 + 一个把 argv 打回来的脚本，端到端跑真进程。
    #[cfg(windows)]
    #[test]
    fn multiline_arg_survives_after_unwrap() {
        // 没有 node 就跳过（CI 上一定有；开发机没有时不该假红）
        let node = node_exe();
        if node == "node" && std::process::Command::new("node").arg("-v").output().is_err() {
            eprintln!("跳过：本机没有 node");
            return;
        }
        let root = std::env::temp_dir().join("uking-launcher-test-e2e");
        std::fs::create_dir_all(&root).unwrap();
        let js = root.join("echoargs.js");
        std::fs::write(&js, "process.stdout.write(JSON.stringify(process.argv.slice(2)));").unwrap();
        let shim = root.join("echoargs.cmd");
        std::fs::write(
            &shim,
            format!("@ECHO off\r\nSET dp0=%~dp0\r\nnode  \"%dp0%\\echoargs.js\" %*\r\n"),
        )
        .unwrap();

        let multiline = "第一行\n第二行";

        // ① 老路子：直接 spawn .cmd —— 必须失败，否则这个 bug 早就不存在了
        let direct = std::process::Command::new(&shim).arg(multiline).output();
        assert!(direct.is_err(), "std 竟然接受了含换行的 .cmd 参数？那本模块的前提变了，请重新评估");

        // ② 新路子：解包后走 node —— 参数必须一字不差地到达
        let l = unwrap_shim(&shim).expect("解包失败");
        let out = std::process::Command::new(&l.program)
            .args(l.full_args(&[multiline.to_string()]))
            .output()
            .expect("node 起不来");
        let got: Vec<String> = serde_json::from_slice(&out.stdout).expect("子进程没吐出 JSON");
        assert_eq!(got, vec![multiline.to_string()], "换行没原样透传");

        let _ = std::fs::remove_dir_all(&root);
    }
}
