//! 沙箱测试的**唯一**入口（只在 `cfg(test)` 下编译）。
//!
//! # 为什么必须只有一份
//!
//! `UKING_TEST_HOME` 是**进程级**环境变量，而 `cargo test` 默认多线程并行跑。
//! 「每个测试模块各自 `static LOCK`」是不够的 —— 那锁的根本不是同一个对象：
//! A 模块跑到一半，B 模块把 env 指到自己的沙箱，A 的后半段就在读别人的目录。
//!
//! 这不是假想。2026-08-08 的实测：`providers.rs` 有一把带注释的权威锁（注释原文
//! 就写着「凡是要改 `UKING_TEST_HOME` 的测试模块，一律锁这一把」），但 `org.rs`
//! 复制 `with_sandbox` 时**连注释一起抄走、却新起了一把本地锁**，`journal.rs` /
//! `aitasks.rs` / `usage.rs` 干脆没锁。结果：默认 `cargo test` 稳定 4 个红
//! （org / journal / aitasks / automation），`--test-threads=1` 则 248 全绿。
//!
//! 🔴 **「把几件事合并进同一个 `#[test]`」挡不住这个。** journal 和 aitasks 的注释
//! 都写了这条自认为的对策，它只挡得住模块**内**的并行，挡不住模块**之间**的。
//! 同一事实（「谁在用沙箱 home」）存在几份就会漂移几份 —— 宪法第 8 条在测试代码里
//! 的现形，代价是「单独跑全绿、一起跑就红」这种最难查的假信号。
//!
//! # 约定
//!
//! 凡是要改 `UKING_TEST_HOME`（或任何进程级环境变量）的 `#[test]`，一律走
//! [`with_sandbox`]，**不要**自己 `set_var`，也不要再起第二把锁。

#![allow(dead_code)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

/// 全进程唯一的沙箱互斥锁。**别再加第二把。**
static SANDBOX_LOCK: Mutex<()> = Mutex::new(());

/// 由沙箱**存档并还原**的进程级环境变量（存档 ≠ 强设，见 [`enter`] / [`enter_raw`]）。
///
/// `HERMES_HOME` 在列表里是有来历的：开发机 shell 里的残留曾让我们把 Hermes 家目录
/// 的结论**测反过一次**（写进了没人读的目录，客户机上 Hermes 全线 404）。测试里一律
/// 清干净 —— 宿主环境不许参与决定断言结果。
///
/// `USERPROFILE` / `HOME` / `APPDATA` / `LOCALAPPDATA` 在列表里是另一段账：`backup.rs`
/// 的用例把它们指到临时目录后**从不还原**，跑完这几条，同进程后面所有测试看到的
/// `USERPROFILE` 都指着一个已经被删掉的目录 —— 而 `uking_home()` 在没有
/// `UKING_TEST_HOME` 时正是读它。这种「上一个用例改坏了全局，下一个用例莫名其妙」
/// 的账最难查，统一由沙箱兜住。
const MANAGED_VARS: &[&str] = &[
    "UKING_TEST_HOME",
    "HERMES_HOME",
    "DSH_HOME",
    "USERPROFILE",
    "HOME",
    "APPDATA",
    "LOCALAPPDATA",
    "UKING_KEEP_SNAPSHOTS",
];

/// 进沙箱跑一段测试：锁 → 建目录 → 改 env → 跑 `f` → 还原。
///
/// - `tag`：沙箱目录后缀，同一个进程里各测试请给不同的 tag。
/// - `dirs`：需要预先建好的子目录（相对 root），如 `&[".uking"]`、`&[".codex"]`。
/// - `f`：拿到沙箱 root。
///
/// **`f` panic 也会还原**（RAII）—— 这一条不是锦上添花：老写法是「`f()` 之后再
/// `remove_var`」，一个断言炸掉就把沙箱 home 泄漏给同进程后面所有测试，于是 1 个
/// 真失败连坐成一串假失败，把真正的病灶埋掉。
pub(crate) fn with_sandbox<T>(tag: &str, dirs: &[&str], f: impl FnOnce(&Path) -> T) -> T {
    let sb = enter(tag, dirs);
    let root = sb.root.clone();
    f(&root)
}

/// [`with_sandbox`] 的 RAII 版：`let _sb = enter(...);` 一行进沙箱，出作用域自动还原。
///
/// 什么时候用它而不是 [`with_sandbox`]：**已有的长用例**。把上百行的测试整体缩进一层
/// 只为套个闭包，既容易改错又让 diff 没法看 —— 而「改造成本高」正是当初各模块宁可
/// 自己复制一份 `with_sandbox` 的原因。两个形式指向同一把锁，随便挑一个顺手的。
pub(crate) fn enter(tag: &str, dirs: &[&str]) -> Sandbox {
    Sandbox::enter(tag, dirs, true)
}

/// 只借锁 + 只负责还原，**不**替你设 `UKING_TEST_HOME`。
///
/// 给那些「自己改 `USERPROFILE`/`APPDATA` 来造家目录」的老用例用（`backup.rs`）：
/// 它们的断言依赖自己那套 home 解析，硬塞一个 `UKING_TEST_HOME` 会改掉解析结果；
/// 但**锁必须是同一把**，否则它们和沙箱用例并行时照样互踩。
pub(crate) fn enter_raw(tag: &str) -> Sandbox {
    Sandbox::enter(tag, &[], false)
}

pub(crate) struct Sandbox {
    root: PathBuf,
    prev: Vec<(&'static str, Option<OsString>)>,
    // 声明在最后：自定义 Drop 跑完（还原 env、删目录）才轮到它释放锁。
    _lock: MutexGuard<'static, ()>,
}

impl Sandbox {
    /// 沙箱根目录（= 这段测试期间的 `UKING_TEST_HOME`）。
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    fn enter(tag: &str, dirs: &[&str], apply: bool) -> Self {
        // 上一个用例 panic 会把锁毒化。毒化只说明「上一个测试炸了」，不代表这把锁
        // 保护的数据坏了（env 和目录我们在 Drop 里已经还原过），照常接手。
        let lock = SANDBOX_LOCK.lock().unwrap_or_else(|e| e.into_inner());

        let root = std::env::temp_dir().join(format!("uking-test-{tag}"));
        // 上次跑崩留下的残渣先清掉，否则「上一轮的文件」会被这一轮读成事实。
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("建沙箱根目录失败");
        for d in dirs {
            std::fs::create_dir_all(root.join(d)).expect("建沙箱子目录失败");
        }

        let prev = MANAGED_VARS
            .iter()
            .map(|k| (*k, std::env::var_os(k)))
            .collect();
        if apply {
            std::env::set_var("UKING_TEST_HOME", &root);
            std::env::remove_var("HERMES_HOME");
            std::env::remove_var("DSH_HOME");
        }

        Self { root, prev, _lock: lock }
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        for (k, v) in &self.prev {
            match v {
                Some(v) => std::env::set_var(k, v),
                None => std::env::remove_var(k),
            }
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 沙箱自己也得可信：env 进得去、出得来，panic 也不许漏。
    ///
    /// 🔴 断言只能写成「出来以后**不再是我这只沙箱**」，不能写成「等于进去前那个值」——
    /// 进去前那个值要在锁外读，而那一刻别的用例正持有沙箱，读到的是**它的**路径，
    /// 于是这条自检自己就是竞态（第一版就这么写的，当场被并行跑抓出来）。
    /// 我这只沙箱的路径全进程唯一，别人永远不会设成它，所以这样断言是稳的。
    #[test]
    fn sets_and_restores_even_on_panic() {
        let mine = with_sandbox("selftest-ok", &[".uking"], |root| {
            assert!(root.join(".uking").is_dir(), "子目录没建出来");
            let live = std::env::var("UKING_TEST_HOME").unwrap();
            assert_eq!(live, root.to_string_lossy(), "沙箱里 env 没指向 root");
            live
        });
        assert!(mine.contains("uking-test-selftest-ok"));
        assert_ne!(
            std::env::var("UKING_TEST_HOME").unwrap_or_default(),
            mine,
            "正常返回没还原"
        );

        let boom = std::panic::catch_unwind(|| {
            with_sandbox("selftest-panic", &[], |root| {
                let live = std::env::var("UKING_TEST_HOME").unwrap();
                assert_eq!(live, root.to_string_lossy());
                panic!("故意炸");
            });
        });
        assert!(boom.is_err(), "catch_unwind 没抓到故意制造的 panic");
        assert!(
            !std::env::var("UKING_TEST_HOME")
                .unwrap_or_default()
                .contains("uking-test-selftest-panic"),
            "panic 之后沙箱 home 漏出来了 —— 后面所有用例都会读到它"
        );
    }

    /// 宿主 shell 里的 `HERMES_HOME` 不许影响断言（那次「测反了」的回归钉子）。
    #[test]
    fn clears_hermes_home_from_host_env() {
        with_sandbox("selftest-hermes", &[], |_| {
            assert!(
                std::env::var_os("HERMES_HOME").is_none(),
                "宿主的 HERMES_HOME 漏进沙箱了"
            );
        });
    }
}
