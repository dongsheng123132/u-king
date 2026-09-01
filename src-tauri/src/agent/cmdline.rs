//! 「看命令」—— 把 GUI 这一轮真实起的子进程，翻成人能看懂、能自己敲的命令行。
//!
//! ## 为什么要有这个模块
//! U-Workspace 对话框底下跑的**就是** `claude` / `codex` 真身。想让客户从 GUI 迁到终端，
//! 唯一诚实的做法是把这行命令**原样**摆出来，而不是把 JSON 事件流伪装成一屏终端输出
//! （那是假的：`-p` 模式没有 TTY，压根没有终端画面可镜像）。
//!
//! ## 两条命令，分清楚，不许混
//! - `display`：**一字不差**的真实命令，含 `--output-format stream-json` 这类只为渲染卡片
//!   而加的参数，也含 `resolve_exe` 解析出的**完整可执行文件路径**（顺带回答「这台机器上
//!   到底哪个 claude 在跑」—— 历史上被 LastAI/MiniMax 抢过 npm 前缀，这行是决定性证据）。
//! - `teach`：终端里**交互式**敲的等价写法。它**不等价于上一行**（去掉了 GUI 专用参数、
//!   改成交互模式会问你要不要批准），前端必须把这句差异说出来，别让人以为是同一条。
//!
//! 纯 std + serde_json，不碰 AppHandle，不 import 其它功能模块。

use serde_json::{json, Value};

/// 提示词能不能内联进 teach 命令：多行 / 太长的贴进命令行只会坑人（shell 会截断或炸引号）。
pub fn inlineable(prompt: &str) -> bool {
    !prompt.contains('\n') && !prompt.contains('\r') && prompt.chars().count() <= 200
}

/// shell 引号：含空格 / 引号 / 反引号等一律双引号包起来并转义内部双引号；空串也要引号。
/// 只用于**显示**（前端复制出去人自己敲），不参与任何真实执行 —— 所以不需要覆盖每种 shell 的转义规则。
fn quote(s: &str) -> String {
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_./:=@+".contains(c)) {
        return s.to_string();
    }
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// 渲染成一行可读命令。
pub fn render(program: &str, args: &[String]) -> String {
    let mut out = quote(program);
    for a in args {
        out.push(' ');
        out.push_str(&quote(a));
    }
    out
}

/// 单个参数在 `display` 里的展示上限（字符数）。
const DISPLAY_ARG_MAX: usize = 72;

/// 同 [`render`]，但把超长参数折叠掉 —— 且**明说折叠了什么、原文多长**。
///
/// 需要它是因为 `--append-system-prompt` 的值是几百字的系统提示（运行环境约束 + 专家设定）。
/// 原样摆出来，「看命令」就从"捅破窗户纸"变成一屏没人看的墙；悄悄砍掉又等于摆了一条跑不通的
/// 假命令。折叠 + 标注原长是唯一两头都不骗人的做法。**只用于 display**：`teach` 里本来就不放
/// 长东西，它必须保持逐字可敲。
pub fn render_folded(program: &str, args: &[String]) -> String {
    let folded: Vec<String> = args
        .iter()
        .map(|a| {
            let n = a.chars().count();
            if n <= DISPLAY_ARG_MAX {
                return a.clone();
            }
            let head: String = a.chars().take(20).collect();
            format!("{head}…〔共 {n} 字，此处折叠〕")
        })
        .collect();
    render(program, &folded)
}

/// 打包成前端 ChatPanel 认的 `kind:"command"` 事件。
///
/// - `exe`：真实起的可执行文件（`resolve_exe` 的结果，可能是全路径）
/// - `real_args`：真实 argv（不含 program）
/// - `bare`：终端里敲的命令名（`claude` / `codex`，不带路径）
/// - `teach_args`：交互式等价写法的参数
/// - `prompt_inlined`：提示词有没有内联进 teach（false 时前端提示「然后把提示词粘进去」）
pub fn event(
    exe: &str,
    real_args: &[String],
    bare: &str,
    teach_args: &[String],
    prompt_inlined: bool,
) -> Value {
    json!({
        "kind": "command",
        "display": render_folded(exe, real_args),
        "teach": render(bare, teach_args),
        "program": bare,
        "prompt_inlined": prompt_inlined,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_only_when_needed() {
        assert_eq!(render("claude", &["--model".into(), "deepseek-v4-pro".into()]), "claude --model deepseek-v4-pro");
        assert_eq!(render("claude", &["写个 hello".into()]), "claude \"写个 hello\"");
        // 内含双引号要转义，否则复制出去粘进终端会断在半路
        assert_eq!(render("c", &["say \"hi\"".into()]), "c \"say \\\"hi\\\"\"");
        // 空参数不能渲染成裸空白（会被 shell 吃掉）
        assert_eq!(render("c", &[String::new()]), "c \"\"");
    }

    #[test]
    fn multiline_prompt_not_inlineable() {
        assert!(inlineable("帮我写个脚本"));
        assert!(!inlineable("第一行\n第二行"));
        assert!(!inlineable(&"啊".repeat(201)));
    }

    /// 长系统提示要折叠，且要**说清楚折叠了多少** —— 悄悄砍掉就成了一条跑不通的假命令。
    /// 短参数一个字都不许动（完整 exe 路径是「这台机器上哪个 claude 在跑」的决定性证据）。
    #[test]
    fn display_folds_long_args_but_says_so() {
        let long = "运".repeat(300);
        // 真实形态：客户机上的 exe 路径本身就可能超过折叠阈值（这条 87 字符），它一个字都不许少 ——
        // 「这台机器上到底哪个 claude 在跑」正是靠它回答的（历史上被 LastAI/MiniMax 抢过 npm 前缀）。
        let exe = "C:/Users/user1/.uking/runtime/node/node_modules/@anthropic-ai/claude-code/bin/claude.exe";
        let out = render_folded(
            exe,
            &["--append-system-prompt".into(), long, "-p".into(), "写个脚本".into()],
        );
        assert!(out.contains(exe), "可执行文件路径不许折叠");
        assert!(out.contains("--append-system-prompt"), "参数名不许折叠");
        assert!(out.contains("共 300 字"), "折叠了却没说原文多长");
        assert!(!out.contains(&"运".repeat(30)), "长参数没折叠");
        assert!(out.contains("写个脚本"), "短参数被误折叠了");
        // 没有超长参数时，两个 render 必须逐字一致
        let plain: Vec<String> = vec!["--model".into(), "deepseek-v4-pro".into()];
        assert_eq!(render_folded("claude", &plain), render("claude", &plain));
    }

    /// teach 绝不能把 GUI 专用参数带上 —— 带上就等于教客户敲一条他根本不该敲的命令。
    #[test]
    fn teach_is_not_display() {
        let ev = event(
            "C:\\npm\\claude.cmd",
            &["--output-format".into(), "stream-json".into(), "-p".into(), "hi".into()],
            "claude",
            &["hi".into()],
            true,
        );
        let display = ev["display"].as_str().unwrap();
        let teach = ev["teach"].as_str().unwrap();
        assert!(display.contains("stream-json"));
        assert!(!teach.contains("stream-json"));
        assert!(!teach.contains("-p"));
        assert_eq!(teach, "claude hi");
    }
}
