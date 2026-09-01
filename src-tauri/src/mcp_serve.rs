//! MCP 服务端 —— 把影核动作原样暴露给 AI（`U-King.exe mcp serve`）。
//!
//! 这是影核协议的第三个面：桌面（GUI 按钮）、CLI（`action run`）之外，AI 从这里进来。
//! **不是第二份实现** —— 每个 MCP 工具就是一个 `actions::run`，契约、确认门禁、
//! 入参校验、乐观并发全部复用同一份核心（宪法第 13 条）。
//!
//! ## 为什么默认只给只读
//! 确认门禁挡的是「顺手误调」：一个脚本或 agent 不会平白无故写 `confirm:true`。
//! 但它挡不住一个铁了心要传的调用方 —— 门禁是**意图声明**，不是权限系统。
//! 所以写动作要 `--allow-write` 才出现在工具表里：**不给的能力才是真的给不出去**。
//! 远程诊断（本 server 的主用途）只需要只读，默认就够用。
//!
//! ## 传输
//! stdio + 行分隔 JSON-RPC 2.0。**stdout 只放协议报文**，日志和动作进度一律走 stderr ——
//! 往 stdout 多打一个字节就会把整条 MCP 连接弄坏（宪法第 14 条）。
//!
//! ## 独立可插拔
//! 只依赖 `actions`，不认识任何业务模块。删它：lib.rs 去 `mod mcp_serve;` + 一个分支。

use serde_json::{json, Value};
use std::io::{BufRead, Write};

/// MCP 协议版本。跟着 Claude Code 当前实现走；对端声明别的版本时我们原样回它自己的，
/// 不为一个诊断用的 server 去做版本协商矩阵。
const PROTOCOL_VERSION: &str = "2024-11-05";

/// 跑一个 stdio MCP server，直到 stdin 关闭。返回进程退出码。
pub fn serve(allow_write: bool) -> i32 {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    eprintln!(
        "[mcp] U-King action server ready (write actions: {})",
        if allow_write { "exposed" } else { "hidden — pass --allow-write to expose" }
    );
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let req: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            // 连 JSON 都不是：按 JSON-RPC 规矩回 -32700，但没有 id 可回时只能记日志。
            Err(e) => {
                eprintln!("[mcp] parse error: {e}");
                continue;
            }
        };
        // 没有 id = 通知（notification），**一个字节都不能回**，否则对端会把它当成
        // 某个请求的应答，之后所有 id 都对不上。
        let Some(id) = req.get("id").cloned() else {
            continue;
        };
        let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let params = req.get("params").cloned().unwrap_or_else(|| json!({}));
        let response = match dispatch(method, params, allow_write) {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err((code, message)) => {
                json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
            }
        };
        let Ok(text) = serde_json::to_string(&response) else { continue };
        if writeln!(out, "{text}").is_err() || out.flush().is_err() {
            break; // 对端走了
        }
    }
    0
}

fn dispatch(method: &str, params: Value, allow_write: bool) -> Result<Value, (i64, String)> {
    match method {
        "initialize" => Ok(json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "u-king", "version": env!("CARGO_PKG_VERSION") }
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tool_list(allow_write) })),
        "tools/call" => call_tool(params, allow_write),
        other => Err((-32601, format!("method not found: {other}"))),
    }
}

/// 动作 id → MCP 工具名。点号换下划线：MCP 工具名在各家实现里的字符集限制不一致，
/// 换成下划线是最保守的选择。真正的稳定标识（原始 Action ID）写进 description，
/// 且 `tools/call` 两种写法都认 —— 契约不因为传输层的字符限制而分叉。
fn tool_name(action_id: &str) -> String {
    action_id.replace('.', "_")
}

fn tool_list(allow_write: bool) -> Vec<Value> {
    crate::actions::list()
        .into_iter()
        .filter(|a| allow_write || a.effect == "read")
        .map(|a| {
            let effect_note = match a.effect.as_str() {
                "read" => "Read-only; changes nothing.".to_string(),
                "destructive" => {
                    "CHANGES THIS MACHINE AND CANNOT BE UNDONE. Requires confirm=true.".to_string()
                }
                _ => "Changes this machine. Requires confirm=true.".to_string(),
            };
            json!({
                "name": tool_name(&a.id),
                "description": format!("{}\n\n{}\n\n{} Action ID: {}", a.title, a.description, effect_note, a.id),
                "inputSchema": a.input_schema.clone()
                    .unwrap_or_else(|| json!({ "type": "object", "additionalProperties": false })),
            })
        })
        .collect()
}

fn call_tool(params: Value, allow_write: bool) -> Result<Value, (i64, String)> {
    let name = params
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or((-32602, "missing tool name".to_string()))?;
    let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));

    // 两种写法都认：原始 id（runtime.driver.inspect）和下划线名（runtime_driver_inspect）。
    let action = crate::actions::list()
        .into_iter()
        .find(|a| a.id == name || tool_name(&a.id) == name)
        .ok_or((-32602, format!("unknown tool: {name}")))?;

    // 没开 --allow-write 时，写动作**根本不在工具表里**；这里再挡一次，
    // 防的是对端拿着上次连接列到的名字直接call（工具表变了它可能不知道）。
    if !allow_write && action.effect != "read" {
        return Err((
            -32602,
            format!("{} is a {} action; restart the server with --allow-write to expose it", action.id, action.effect),
        ));
    }

    // 动作进度走 stderr：stdout 是 MCP 报文通道，混进去就废了。
    let sink = |m: &str| eprintln!("[mcp][progress] {m}");
    match crate::actions::run_with_progress(&action.id, args, &sink) {
        Ok(value) => Ok(json!({
            "content": [{ "type": "text", "text": serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".into()) }],
            "isError": false
        })),
        // 动作层的失败（confirmation_required / conflict / invalid_input）作为**工具结果**
        // 回去而不是 JSON-RPC 错误：AI 需要读到这句话才知道下一步该怎么做，
        // 而 JSON-RPC error 在多数客户端里只会显示成一句「工具调用失败」。
        Err(e) => Ok(json!({
            "content": [{ "type": "text", "text": e }],
            "isError": true
        })),
    }
}
