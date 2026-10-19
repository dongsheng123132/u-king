//! 本机「正在提供服务的本地 AI 应用」探测：LM Studio / Jan / GPT4All / KoboldCpp / LocalAI / Msty。
//!
//! ## 为什么按端口探，而不是按卸载表认
//! 这几款的安装形态各异（便携解压、用户级安装、Docker、纯 CLI），卸载表形状没有逐一实测过，
//! 照猜写规则等于编造。它们共同的、**官方文档写明**的事实是：开了本地服务就会在固定默认端口上
//! 暴露 OpenAI 兼容的 `GET /v1/models`。所以判据是「这个端口真的回了一份合法的模型列表」——
//! 这是**此刻能用**的证据，比「装没装」更接近客户要的答案（对标 `localllm` 的 readiness 约定）。
//!
//! ## 诚实边界（别在界面上说过头）
//! - 只认**默认端口**。用户改了端口就探不到；这是「没探到」，不是「没装」。
//! - 端口只能说明「有个 OpenAI 兼容服务在这儿」，**不能证明它就是那个应用**（8080 尤其泛）。
//!   所以结果里带 `likely` 语义：`label` 是「默认端口对应的应用」，`note` 写明依据。
//! - Ollama（11434）与 llama.cpp 已由 `localllm` 的引擎层负责，这里不重复。
//!
//! ## 独立可插拔
//! 纯 `std` + `serde`，不 import 任何功能模块；HTTP 用裸 TCP 手写一个 GET，不引依赖、不走代理
//! （127.0.0.1 直连，系统代理不该介入）。删本模块只动 lib.rs 的 `mod` 与调用点。

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use serde::Serialize;

const CONNECT_TIMEOUT: Duration = Duration::from_millis(300);
const IO_TIMEOUT: Duration = Duration::from_millis(1500);
/// 模型列表再大也不会过这个量；防御「端口上是个会一直吐数据的东西」。
const MAX_BODY: usize = 256 * 1024;

/// 一个待探测的默认端口。
#[derive(Debug, Clone, Copy)]
pub struct Candidate {
    pub id: &'static str,
    pub label: &'static str,
    pub port: u16,
}

/// 默认端口表。每一条都来自对应项目的官方文档（2026-10-19 核）：
/// LM Studio `localhost:1234/v1`、Jan `127.0.0.1:1337/v1`、GPT4All `localhost:4891/v1`、
/// KoboldCpp `localhost:5001/v1`、LocalAI `:8080/v1`、Msty 桌面版 `localhost:10000/v1`、
/// Magpie 网关 `127.0.0.1:3425/v1`。
pub const CANDIDATES: &[Candidate] = &[
    Candidate { id: "lmstudio", label: "LM Studio", port: 1234 },
    Candidate { id: "jan", label: "Jan", port: 1337 },
    Candidate { id: "gpt4all", label: "GPT4All", port: 4891 },
    Candidate { id: "koboldcpp", label: "KoboldCpp", port: 5001 },
    Candidate { id: "localai", label: "LocalAI", port: 8080 },
    Candidate { id: "msty", label: "Msty", port: 10000 },
    // Magpie 本地网关（官方 README：`http://127.0.0.1:3425/v1`，客户端 key 填占位串 `magpie`）。
    // 智能路由组（`group/<名字>`）就是通过这个网关当“模型名”用的。`/v1/models` 是否列出路由组
    // 本轮**未实测**——探不到只说明网关没开或不回模型列表，界面别说成“没装”。
    Candidate { id: "magpie-gateway", label: "Magpie 网关", port: 3425 },
];

/// 一个探到的、正在提供 OpenAI 兼容服务的本地应用。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LocalAiService {
    pub id: String,
    pub label: String,
    pub port: u16,
    /// 可直接当 OpenAI 兼容供应商 baseUrl 用的地址。
    pub endpoint: String,
    /// `GET /v1/models` 返回的模型 id（可能为空：服务在、但还没加载模型）。
    pub models: Vec<String>,
    /// 说人话的依据，界面原样显示，避免把「默认端口上有服务」说成「确认是它」。
    pub note: String,
}

/// 探测所有默认端口（并发，总耗时约一个超时）。结果按 `CANDIDATES` 顺序。
pub fn probe_all() -> Vec<LocalAiService> {
    probe(CANDIDATES)
}

/// 可注入候选表的版本（测试用临时端口）。
pub fn probe(candidates: &[Candidate]) -> Vec<LocalAiService> {
    std::thread::scope(|s| {
        let handles: Vec<_> = candidates
            .iter()
            .map(|c| s.spawn(move || probe_one(*c)))
            .collect();
        handles
            .into_iter()
            .filter_map(|h| h.join().ok().flatten())
            .collect()
    })
}

fn probe_one(c: Candidate) -> Option<LocalAiService> {
    let body = http_get_local(c.port, "/v1/models")?;
    let models = parse_models(&body)?;
    Some(LocalAiService {
        id: c.id.to_string(),
        label: c.label.to_string(),
        port: c.port,
        endpoint: format!("http://127.0.0.1:{}/v1", c.port),
        models,
        note: format!(
            "{} 默认端口 {} 上有 OpenAI 兼容服务在响应（按端口判断，若你改过端口则不会出现在这里）",
            c.label, c.port
        ),
    })
}

/// 裸 TCP 的 HTTP/1.0 GET，只返回 2xx 的 body。任何失败都是 `None`（= 没探到，不是报错）。
fn http_get_local(port: u16, path: &str) -> Option<String> {
    let addr: SocketAddr = ([127, 0, 0, 1], port).into();
    let mut stream = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT).ok()?;
    stream.set_read_timeout(Some(IO_TIMEOUT)).ok()?;
    stream.set_write_timeout(Some(IO_TIMEOUT)).ok()?;
    let req = format!("GET {path} HTTP/1.0\r\nHost: 127.0.0.1:{port}\r\nAccept: application/json\r\nConnection: close\r\n\r\n");
    stream.write_all(req.as_bytes()).ok()?;

    let mut raw = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                raw.extend_from_slice(&chunk[..n]);
                if raw.len() > MAX_BODY {
                    return None;
                }
            }
            // 读超时：已经拿到的就是全部（有的服务不主动关连接）
            Err(_) => break,
        }
    }
    split_ok_body(&raw)
}

/// 从原始响应里取 2xx 状态的 body；非 2xx / 格式不对 → `None`。
fn split_ok_body(raw: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(raw);
    let (head, body) = text.split_once("\r\n\r\n")?;
    let status_line = head.lines().next()?;
    let code: u16 = status_line.split_whitespace().nth(1)?.parse().ok()?;
    if !(200..300).contains(&code) {
        return None;
    }
    Some(body.to_string())
}

/// 合法的 OpenAI 模型列表：`{"data":[{"id":"..."}...]}`（`object` 字段各家不一，不强求）。
/// 不是这个形状 = 这个端口上不是我们要找的东西 → `None`。
fn parse_models(body: &str) -> Option<Vec<String>> {
    let v: serde_json::Value = serde_json::from_str(body.trim()).ok()?;
    let data = v.get("data")?.as_array()?;
    Some(
        data.iter()
            .filter_map(|m| m.get("id").and_then(|x| x.as_str()))
            .map(str::to_string)
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    /// 起一个只应答一次的假服务，返回它的临时端口。
    fn serve_once(response: &'static str) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = listener.accept() {
                let mut buf = [0u8; 1024];
                let _ = s.read(&mut buf);
                let _ = s.write_all(response.as_bytes());
            }
        });
        port
    }

    fn cand(port: u16) -> Candidate {
        Candidate { id: "demo", label: "Demo", port }
    }

    #[test]
    fn detects_valid_openai_model_list() {
        let port = serve_once("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"object\":\"list\",\"data\":[{\"id\":\"demo-model-a\"},{\"id\":\"demo-model-b\"}]}");
        let found = probe(&[cand(port)]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].models, vec!["demo-model-a", "demo-model-b"]);
        assert_eq!(found[0].endpoint, format!("http://127.0.0.1:{port}/v1"));
    }

    #[test]
    fn service_without_models_is_still_reported() {
        // 服务在、还没加载模型：要如实告诉用户「在跑但空」，不是「没探到」
        let port = serve_once("HTTP/1.1 200 OK\r\n\r\n{\"data\":[]}");
        let found = probe(&[cand(port)]);
        assert_eq!(found.len(), 1);
        assert!(found[0].models.is_empty());
    }

    #[test]
    fn non_openai_json_or_html_is_not_reported() {
        let html = serve_once("HTTP/1.1 200 OK\r\n\r\n<html>welcome</html>");
        let other_json = serve_once("HTTP/1.1 200 OK\r\n\r\n{\"status\":\"ok\"}");
        assert!(probe(&[cand(html)]).is_empty());
        assert!(probe(&[cand(other_json)]).is_empty());
    }

    #[test]
    fn non_2xx_is_not_reported() {
        let port = serve_once("HTTP/1.1 404 Not Found\r\n\r\n{\"data\":[{\"id\":\"x\"}]}");
        assert!(probe(&[cand(port)]).is_empty());
    }

    #[test]
    fn closed_port_is_not_reported() {
        // 绑定后立刻放掉，端口大概率无人监听
        let port = TcpListener::bind("127.0.0.1:0").expect("bind").local_addr().expect("addr").port();
        assert!(probe(&[cand(port)]).is_empty());
    }

    #[test]
    fn ids_and_ports_are_unique_and_not_ollama() {
        let mut ids: Vec<_> = CANDIDATES.iter().map(|c| c.id).collect();
        let mut ports: Vec<_> = CANDIDATES.iter().map(|c| c.port).collect();
        ids.sort_unstable();
        ports.sort_unstable();
        ids.dedup();
        ports.dedup();
        assert_eq!(ids.len(), CANDIDATES.len(), "id 不许重复");
        assert_eq!(ports.len(), CANDIDATES.len(), "端口不许重复");
        // Ollama / llama.cpp 归 localllm 引擎层，这里重复探会在界面上出两份
        assert!(!CANDIDATES.iter().any(|c| c.port == 11434 || c.id == "ollama" || c.id == "llamacpp"));
    }
}
