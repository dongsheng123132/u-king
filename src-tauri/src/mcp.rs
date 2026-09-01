//! MCP 连接器 —— **功能已于 2026-08-11 删除，本模块只剩「卸载清理」这一条用途**。
//!
//! 原来它给「对话大脑」CLI 挂 MCP server（文件系统 / 深度思考 / 长期记忆 / 浏览器操控 4 个精选
//! 连接器）。2026-07-14 产品决策把入口从导航摘掉（概念对小白太重、通用连接器价值低、
//! 且只对 Claude 生效），2026-08-11「简化第二刀」把 UI、命令、影核动作 `runtime.connector.*`
//! 一并删掉。
//!
//! **为什么不整个文件删掉**：装过连接器的存量客户，机器上 `~/.claude.json` 里仍然留着那几条
//! 记录。`cleanup.rs`（安全卸载 / 逐项清理）要能把它们列出来并删干净 —— 功能没了不等于
//! 客户机上的痕迹没了。**删掉这里 = 那些客户从此再也清不掉我们留下的东西**，这正是
//! 「安全卸载」那一页承诺过不会发生的事。
//!
//! 所以本模块现在只保留两件事，且**只对 Claude**（当年也只有 Claude 真挂上过）：
//! - `CURATED` —— 认得出哪些 MCP server 是我们加的（不碰用户自己加的）
//! - `list_installed()` / `remove()` —— 列出来、删掉
//!
//! 🔴 **不要在这里重新长出功能**。要恢复连接器请从 git 历史取回完整版
//! （`git log -- src-tauri/src/mcp.rs`，删除前的最后一版是 0.9.95），别在清理模块上加 add。
//!
//! ⚠️ providers.rs 切驱动时保留 `[mcp_servers.*]` 的逻辑（`extract_mcp_servers`）**照旧不能删** ——
//! 那份 config.toml 是被整文件覆盖的，不捞出来带回去，客户**自己**加的连接器会被我们切一次驱动
//! 就全没了。跟本模块无关，它守的是用户自己的东西。单测：
//! `extract_mcp_servers_keeps_user_connectors_and_nothing_else`。
//!
//! **独立可插拔**：纯函数 + 只复用 `installer::search_paths`。不碰 AppHandle，不含任何 Key。

use std::path::PathBuf;
use std::process::Command;

use crate::installer::{portable_node_dir, search_paths};

/// 我们当年装过的连接器 id + 名字。**只用来认领**：清理时凭它区分「U-King 加的」和
/// 「用户自己加的」——后者一个字节都不许动。
///
/// 原结构还有 desc/category/command/args/needs_dir 等字段，那些是 add 用的，随功能一起删了。
pub struct Connector {
    pub id: &'static str,
    pub name: &'static str,
}

/// 历史上 U-King 提供过的 4 个精选连接器。**这份名单是冻结的** ——
/// 它描述的是「过去可能被写进客户机的东西」，不是「现在提供什么」，所以永远不该再增行。
pub const CURATED: &[Connector] = &[
    Connector { id: "filesystem", name: "文件系统" },
    Connector { id: "sequential-thinking", name: "深度思考" },
    Connector { id: "memory", name: "长期记忆" },
    Connector { id: "playwright", name: "浏览器操控" },
];

/// 把命令名解析成 search_paths 里真实存在的可执行文件全路径（与 agent/claude.rs 同源思路）。
fn resolve(program: &str) -> String {
    // Windows：npm 全局装的是 claude.cmd，必须先试 .cmd/.exe（否则会命中同名无扩展的
    // shell 脚本，Command::new 起它报 os error 193「不是有效 Win32 应用」）。
    #[cfg(windows)]
    let exts: &[&str] = &[".cmd", ".exe", ".bat", ""];
    #[cfg(not(windows))]
    let exts: &[&str] = &[""];
    for dir in search_paths(portable_node_dir().as_deref()) {
        for e in exts {
            let p = dir.join(format!("{program}{e}"));
            if p.is_file() {
                return p.to_string_lossy().into_owned();
            }
        }
    }
    program.to_string()
}

fn base(program: &str) -> Command {
    let mut c = Command::new(resolve(program));
    // PATH 前置 search_paths，让 claude 内部再起 npx/node 也能找到
    let mut all: Vec<PathBuf> = search_paths(portable_node_dir().as_deref());
    if let Some(cur) = std::env::var_os("PATH") {
        all.extend(std::env::split_paths(&cur));
    }
    if let Ok(j) = std::env::join_paths(all) {
        c.env("PATH", j);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    c
}

/// 列出 Claude Code 上已配的 MCP server 名（给清理页做足迹扫描）。
///
/// Claude 没有 `mcp list --json`，只能解析 "name: …" 行。找不到 claude / 没装过 → 空表，
/// 这对清理页是正确行为（没有痕迹可清）。
pub fn list_installed() -> Vec<String> {
    let out = match base("claude").args(["mcp", "list"]).output() {
        Ok(o) => o,
        Err(_) => return vec![],
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let mut names = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("Checking") || line.starts_with("No MCP") {
            continue;
        }
        if let Some(idx) = line.find(": ") {
            names.push(line[..idx].trim().to_string());
        }
    }
    names
}

/// 从 Claude Code 上摘掉一个连接器（清理页的「删除」动作）。
///
/// `--scope user` 必须对上当年 add 时用的那个 scope，否则删不掉却报成功。
pub fn remove(id: &str) -> Result<String, String> {
    crate::ulog::write("mcp", &format!("[claude] 清理遗留连接器 id={id}"));
    let out = base("claude")
        .args(["mcp", "remove", id, "--scope", "user"])
        .output()
        .map_err(|e| format!("起 claude 失败: {e}"))?;
    if out.status.success() {
        Ok("已删除".into())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        crate::ulog::write("mcp", &format!("[claude] 清理 id={id} 失败：{}", err.trim()));
        Err(if err.trim().is_empty() { "删除失败".into() } else { err.trim().to_string() })
    }
}
