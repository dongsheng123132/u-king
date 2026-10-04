//! Token 压缩机（rtk）退役壳 —— 功能已删，但客户机上可能还挂着我们写的 Claude Code hook。
//! 1. `run_hook_passthrough`：`U-King.exe rtk-hook` = 读完 stdin 丢掉、stdout 零字节、退出码 0
//!    （Claude Code 读到空输出 = 按原命令执行）。**入口必须保留**：hook 挂在客户每条 Bash 命令上，
//!    删了这个分支 `rtk-hook` 参数会落进 GUI 启动，每条命令都把窗口顶到前台。
//! 2. `unhook_once`：启动时摘掉 settings.json 里我们的 hook、删掉我们自己的 rtk 文件；
//!    摘不干净就**不删文件**，也绝不覆盖用户原文件。至少保留到 2026-12-31，之后再评估删除本模块。

use serde_json::Value;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// 原子写由组合根注入（lib.rs 传 `providers::atomic_write`），本模块不直接依赖 providers（模块耦合闸门）。
type WriteFn<'a> = &'a dyn Fn(&PathBuf, &[u8]) -> Result<(), String>;

/// 读完 stdin 丢弃；`_out` 永远不写。拆出来是为了能脱离真 stdin 测「输出为空」。
fn passthrough<R: Read, W: Write>(mut input: R, _out: W) -> i32 {
    let _ = std::io::copy(&mut input, &mut std::io::sink());
    0
}

/// `rtk-hook` 入口。读 stdin 放后台线程、最多等 5 秒：对面不关管道也照样放行，不拖住客户的命令。
pub fn run_hook_passthrough() -> i32 {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(passthrough(std::io::stdin().lock(), std::io::sink()));
    });
    let _ = rx.recv_timeout(std::time::Duration::from_secs(5));
    0
}

/// 是不是我们（或 rtk）装的 hook 命令：新写法 `"<exe>" rtk-hook`（独立参数，别被目录名骗），
/// 或老写法 `"<path>/rtk(.exe)" hook claude`（可执行文件名必须是 rtk，免得误删别家的 `hook claude`）。
fn is_our_hook(cmd: &str) -> bool {
    cmd.split_whitespace().any(|t| t.trim_matches('"') == "rtk-hook")
        || cmd.find("hook claude").is_some_and(|i| {
            let exe = cmd[..i].trim().trim_matches('"').replace('\\', "/");
            matches!(exe.rsplit('/').next(), Some("rtk" | "rtk.exe"))
        })
}

/// 摘掉 `hooks.PreToolUse` 里我们的条目，返回摘了几条；只清理**因我们摘除而变空**的容器。
fn strip_our_hooks(root: &mut Value) -> usize {
    let (mut n, mut drop_hooks) = (0, false);
    if let Some(hooks) = root.get_mut("hooks").and_then(Value::as_object_mut) {
        let mut drop_pre = false;
        if let Some(pre) = hooks.get_mut("PreToolUse").and_then(Value::as_array_mut) {
            pre.retain_mut(|entry| {
                let Some(hs) = entry.get_mut("hooks").and_then(Value::as_array_mut) else { return true };
                let before = hs.len();
                hs.retain(|h| !h.get("command").and_then(Value::as_str).is_some_and(is_our_hook));
                n += before - hs.len();
                !(hs.len() < before && hs.is_empty())
            });
            drop_pre = n > 0 && pre.is_empty();
        }
        if drop_pre {
            hooks.remove("PreToolUse");
        }
        drop_hooks = n > 0 && hooks.is_empty();
    }
    if let (true, Some(o)) = (drop_hooks, root.as_object_mut()) { o.remove("hooks"); }
    n
}

/// settings.json 这一半：备份 → 摘除 → 原子替换 → 读回确认。解析不了 / 写不进 = Err，原文件不动。
fn unhook_settings(settings: &PathBuf, write: WriteFn) -> Result<String, String> {
    let bytes = std::fs::read(settings).map_err(|e| format!("读 settings.json 失败，未改动：{e}"))?;
    let mut root: Value = serde_json::from_slice(&bytes).map_err(|e| format!("settings.json 解析失败，未改动：{e}"))?;
    let n = strip_our_hooks(&mut root);
    if n == 0 {
        return Ok("settings.json 里没有我们的 hook".into());
    }
    let bak = settings.with_extension("json.uking-rtk-bak");
    if !bak.exists() {
        std::fs::write(&bak, &bytes).map_err(|e| format!("备份失败，未改动：{e}"))?;
    }
    let mut out = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
    if bytes.ends_with(b"\n") {
        out.push('\n');
    }
    write(settings, out.as_bytes())?;
    let mut back = std::fs::read(settings).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok());
    match back.as_mut().map(strip_our_hooks) {
        Some(0) => Ok(format!("已摘除 {n} 条旧 Token 压缩机 hook")),
        _ => Err("摘除后读回校验没过（原文件见 settings.json.uking-rtk-bak）".into()),
    }
}

/// 摘 hook + 删我们自己的文件。**不动 PATH**（shims 目录与 CLI 守卫共用）。幂等。
/// `Ok(说明)` 给 ulog；`Err` = 没删任何文件，下次启动再试。
pub fn unhook_once(home: &Path, write: WriteFn) -> Result<String, String> {
    let settings = home.join(".claude").join("settings.json");
    let note = if settings.exists() { unhook_settings(&settings, write)? } else { "没有 settings.json".into() };
    // 走到这里 = hook 已确认不在，才删程序（否则旧 hook 指着被删的程序，反而更糟）。
    let uking = home.join(".uking");
    let _ = std::fs::remove_dir_all(uking.join("tools").join("rtk"));
    for name in ["rtk", "rtk.cmd"] {
        let _ = std::fs::remove_file(uking.join("shims").join(name));
    }
    Ok(note)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testsandbox::with_sandbox;

    const NEW: &str = r#""C:/x/U-King.exe" rtk-hook"#;
    const OLD: &str = r#""C:/Users/a b/.uking/tools/rtk/rtk.exe" hook claude"#;
    /// 目录名含 rtk-hook 但不是独立参数（第一版判据被它骗过）：不许误删。
    const TRAP: &str = r#""C:/uking-test-rtk-hook-heal/x.exe" run"#;

    fn w(p: &PathBuf, d: &[u8]) -> Result<(), String> { std::fs::write(p, d).map_err(|e| e.to_string()) }
    fn write(root: &Path, json: &str) -> PathBuf {
        let p = root.join(".claude").join("settings.json");
        std::fs::write(&p, json).unwrap();
        p
    }
    fn hook(cmd: &str) -> String {
        format!(r#"{{"matcher":"Bash","hooks":[{{"type":"command","command":{}}}]}}"#, serde_json::json!(cmd))
    }
    fn load(p: &Path) -> Value { serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap() }
    fn bak(p: &Path) -> PathBuf { p.with_extension("json.uking-rtk-bak") }

    #[test]
    fn no_settings_file_is_noop() {
        with_sandbox("rtk-retire-nofile", &[".claude"], |r| {
            assert!(unhook_once(r, &w).is_ok() && !r.join(".claude/settings.json").exists());
        });
    }

    #[test]
    fn removes_new_and_old_style_keeps_others_and_never_overwrites_backup() {
        with_sandbox("rtk-retire-mixed", &[".claude"], |r| {
            let src = format!(
                r#"{{"env":{{"K":"v"}},"hooks":{{"PreToolUse":[{},{},{},{},{}],"Stop":[{}]}}}}"#,
                hook(NEW), hook("other-tool --pre"), hook(OLD), hook("foo hook claude"), hook(TRAP), hook("bye")
            );
            let p = write(r, &src);
            std::fs::write(bak(&p), "SENTINEL").unwrap();
            assert!(unhook_once(r, &w).unwrap().contains("2 条"));
            let v = load(&p);
            let left: Vec<&str> = v["hooks"]["PreToolUse"].as_array().unwrap().iter()
                .map(|e| e["hooks"][0]["command"].as_str().unwrap()).collect();
            assert_eq!(left, ["other-tool --pre", "foo hook claude", TRAP], "别家的条目一条不许动");
            assert!(v["env"]["K"] == "v" && v["hooks"]["Stop"].is_array());
            assert_eq!(std::fs::read_to_string(bak(&p)).unwrap(), "SENTINEL");
        });
    }

    #[test]
    fn empty_containers_cleaned_then_second_call_is_byte_identical() {
        with_sandbox("rtk-retire-empty", &[".claude"], |r| {
            let src = format!(r#"{{"model":"x","hooks":{{"PreToolUse":[{}]}}}}"#, hook(NEW));
            let p = write(r, &src);
            unhook_once(r, &w).unwrap();
            let after = std::fs::read(&p).unwrap();
            let v = load(&p);
            assert!(v.get("hooks").is_none() && v["model"] == "x", "{v}");
            assert_eq!(std::fs::read_to_string(bak(&p)).unwrap(), src, "备份是原文");
            assert!(unhook_once(r, &w).unwrap().contains("没有我们的"));
            assert_eq!(std::fs::read(&p).unwrap(), after);
        });
    }

    #[test]
    fn invalid_json_leaves_file_bytes_and_our_files_untouched() {
        with_sandbox("rtk-retire-badjson", &[".claude", ".uking/tools/rtk"], |r| {
            let p = write(r, "{ \"hooks\": [ oops rtk-hook");
            std::fs::write(r.join(".uking/tools/rtk/rtk.exe"), "x").unwrap();
            assert!(unhook_once(r, &w).is_err());
            assert_eq!(std::fs::read_to_string(&p).unwrap(), "{ \"hooks\": [ oops rtk-hook");
            assert!(!bak(&p).exists() && r.join(".uking/tools/rtk/rtk.exe").exists());
        });
    }

    #[test]
    fn deletes_only_our_files() {
        with_sandbox("rtk-retire-files", &[".uking/tools/rtk", ".uking/shims"], |r| {
            for f in [".uking/tools/rtk/rtk.exe", ".uking/shims/rtk", ".uking/shims/rtk.cmd", ".uking/shims/claude.cmd"] {
                std::fs::write(r.join(f), "x").unwrap();
            }
            unhook_once(r, &w).unwrap();
            assert!(!r.join(".uking/tools/rtk").exists() && !r.join(".uking/shims/rtk").exists());
            assert!(!r.join(".uking/shims/rtk.cmd").exists() && r.join(".uking/shims/claude.cmd").exists());
        });
    }

    #[test]
    fn passthrough_outputs_nothing_for_any_stdin() {
        let inputs: Vec<Vec<u8>> =
            vec![vec![], br#"{"tool_input":{"command":"ls"}}"#.to_vec(), vec![0xff, 0xfe, 0x00, 0x80], vec![b'a'; 1 << 20]];
        for input in &inputs {
            let mut out: Vec<u8> = vec![];
            assert_eq!(passthrough(&input[..], &mut out), 0);
            assert!(out.is_empty());
        }
    }
}
