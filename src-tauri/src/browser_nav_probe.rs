//! 浏览器子窗口「后退 / 前进 / 刷新」到底生没生效 —— 无头取证（需求榜 P0 #5 的硬那半边）。
//!
//! ## 这条跑道为什么非有不可
//!
//! `browser_nav` 的三个按钮走的是 `w.eval("history.back()")`。校验层（`validate_nav`）和
//! 「窗口在不在」有单测盖住，但**「eval 打到一个外部页面上，那个页面到底导航了没有」
//! 一个字节都不在动作表里** —— 需求榜把它记成 P0 发版阻塞，理由是「只能真机点」。
//!
//! 其实不必。要证的不是「按钮好不好按」，是「那句 eval 真的让页面回退了」，
//! 而这件事有一个**不用看、不用点**的判据：`WebviewWindow::url()` 读回真实地址。
//! A → B → back → 地址回到 A，就是决定性证据。
//!
//! ## 三条不许破的规矩（都是为了不打扰用户）
//!
//! 1. **窗口 `visible(false)`**：整个过程屏幕上什么都不出现，不抢前台、不动鼠标。
//!    本机截屏做 UI 验证是禁的（会拍到别人的窗口，出过隐私事故）—— 这条跑道压根不截屏。
//! 2. **不联网**：起一个 std 写的本地 HTTP 服务喂两个页面。既不依赖外网，
//!    又确实是「外部页面」（`http://127.0.0.1:…`，不是 `app://`），跟真实使用同构。
//! 3. **不碰用户那个 U-King**：调用方必须跳过单实例插件（否则第二个实例会把
//!    用户的窗口顶到前面 —— 那正是最不该发生的事）。
//!
//! ## 它验得到什么、验不到什么
//!
//! - ✅ 验：`history.back/forward` 真的改变了页面地址；`location.reload()` 真的**重新发了请求**
//!   （本地服务数请求次数，比「地址没变」有力得多 —— 刷新本来地址就不变）。
//!   走的是 `browser_nav` 里**同一行 eval**。
//! - ❌ 验不到：前端那三个按钮有没有正确接到 `browser_nav`（那是 `invoke` 的接线，
//!   得在真界面上点）。所以 P0 #5 只清掉了硬的那半边，剩下的接线仍需人点一次 —— 别声称全做完了。

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

/// 两个页面被请求了几次。`reload` 的证据就靠它：地址不变，但服务端会再收到一次请求。
pub struct Hits {
    pub a: AtomicU32,
    pub b: AtomicU32,
}

/// 起一个只服务两个页面的本地 HTTP 服务，返回 (端口, 命中计数)。
///
/// 纯 std、单线程顺序处理够用（这条跑道一次就几个请求）。端口交给系统分配（`:0`），
/// 免得跟客户机上任何东西撞port。
pub fn serve() -> std::io::Result<(u16, Arc<Hits>)> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    let hits = Arc::new(Hits { a: AtomicU32::new(0), b: AtomicU32::new(0) });
    let h = hits.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let _ = handle(stream, &h);
        }
    });
    Ok((port, hits))
}

fn handle(mut s: TcpStream, hits: &Hits) -> std::io::Result<()> {
    let mut buf = [0u8; 1024];
    let n = s.read(&mut buf)?;
    let req = String::from_utf8_lossy(&buf[..n]);
    let path = req.split_whitespace().nth(1).unwrap_or("/");
    let (title, body) = if path.starts_with("/b") {
        hits.b.fetch_add(1, Ordering::Relaxed);
        ("B", "<h1>page B</h1>")
    } else {
        hits.a.fetch_add(1, Ordering::Relaxed);
        ("A", "<h1>page A</h1>")
    };
    // no-store：不然 reload 可能命中缓存，服务端收不到第二次请求 —— 那会让
    // 「刷新到底有没有真的发生」这条断言变成假红（跑道骗人比功能坏更贵）。
    let html = format!("<!doctype html><meta charset=utf-8><title>{title}</title>{body}");
    let resp = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nCache-Control: no-store, max-age=0\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}",
        html.len()
    );
    s.write_all(resp.as_bytes())?;
    s.flush()
}

/// 轮询等 `url()` 变成期望值。webview 的导航是异步的，**必须轮询等**，
/// 写死 `sleep(500ms)` 那种在冷机 / 忙机上必翻车（这个坑本仓库连摔过三次）。
pub fn wait_url<F: Fn() -> String>(get: F, want_contains: &str, timeout_ms: u64) -> Option<String> {
    let step = 50;
    let mut waited = 0;
    loop {
        let u = get();
        if u.contains(want_contains) {
            return Some(u);
        }
        if waited >= timeout_ms {
            return None;
        }
        std::thread::sleep(std::time::Duration::from_millis(step));
        waited += step;
    }
}

/// 同上，等一个计数涨上去（给 reload 用）。
pub fn wait_count(get: impl Fn() -> u32, at_least: u32, timeout_ms: u64) -> bool {
    let step = 50;
    let mut waited = 0;
    loop {
        if get() >= at_least {
            return true;
        }
        if waited >= timeout_ms {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(step));
        waited += step;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 本地服务真的能起、两个页面分得开、命中数会涨 —— 跑道自己的地基先立住。
    #[test]
    fn local_pages_are_served_and_counted() {
        let (port, hits) = serve().expect("起不来本地服务");
        let get = |p: &str| {
            let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
            s.write_all(format!("GET {p} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes()).unwrap();
            let mut out = String::new();
            let _ = s.read_to_string(&mut out);
            out
        };
        assert!(get("/a").contains("page A"));
        assert!(get("/b").contains("page B"));
        assert!(get("/b").contains("page B"));
        assert_eq!(hits.a.load(Ordering::Relaxed), 1);
        assert_eq!(hits.b.load(Ordering::Relaxed), 2, "reload 的判据就靠这个计数");
        // 🔴 缓存必须关掉，否则 reload 可能命中缓存、服务端收不到第二次请求 → 假红
        assert!(get("/a").contains("no-store"));
    }

    /// 等待器必须**会超时返回 None**，不能永远挂着 —— 跑道挂死比跑道报错更难查。
    #[test]
    fn waiters_time_out_instead_of_hanging() {
        assert!(wait_url(|| "about:blank".into(), "never", 120).is_none());
        assert!(!wait_count(|| 0, 1, 120));
        // 已经满足时必须立刻返回，不能白等一个超时
        assert!(wait_url(|| "http://x/a".into(), "/a", 5_000).is_some());
        assert!(wait_count(|| 3, 1, 5_000));
    }
}
