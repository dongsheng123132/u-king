//! 办公文档动作核心 —— 把已经在跑的那批技能脚本**升格**成影核动作。
//!
//! ## 为什么是「升格」而不是「重写」
//! `uking-office-edit` 那套「改现有 Word/PPT/Excel 不丢格式」是半年攒出来的
//! （未改部件直接复制原始压缩字节 = 字节级相同；真实 Word 一个段落最多能拆成 103 个
//! `<w:t>`，必须按段落拼接再匹配）。**脚本本身就是实现**，这里只给它稳定 ID、
//! 入参校验和确认门禁。重写一份 Rust 版 = 同一事实存在两份，下次改必然只改一边。
//!
//! ## 为什么办公动作值得进动作表
//! 动作表里现有 56 个动作全是**设备级**的（切驱动、装工具、看用量）—— 它们回答
//! 「这台机器怎么样」。而客户真正要 AI 干的活是**文档级**的：把这份合同的甲方改掉、
//! 把这份 PDF 里的条款读出来。这些能力今天只存在于技能包里，也就是说：
//! **只有装了技能包的 AI 会用，走 CLI / MCP / 远端影子进来的一律不知道有这回事。**
//!
//! ## 粒度：不要把工具栏一比一翻译成 JSON
//! 登记的是**对文档的变换**（read / edit / export），不是「点了哪个按钮」——
//! 按钮是界面动作，宪法第 13 条明说不进核心。把每个格式按钮都变成动作，
//! 动作表会撑到几百个，AI 反而选不动（那正是配方层存在的理由：少量高层意图 + 底层原子动作）。
//!
//! 纯函数、零 `AppHandle`；只依赖 `installer` 这个公共助手层（node/python 定位、PATH 注入）。

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{json, Value};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn home_dir() -> PathBuf {
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

/// 技能脚本在盘上的位置。**读同步下来的那一份**（`~/.uking/skills/<包名>/scripts/<脚本>`），
/// 不是 exe 里内嵌的那份 —— 内嵌的那份没有解压出来的路径可以交给 node/python。
fn script(pack: &str, file: &str) -> PathBuf {
    home_dir().join(".uking").join("skills").join(pack).join("scripts").join(file)
}

/// node 可执行：优先便携版，否则赌 PATH（下面 `inject_path` 已把 `search_paths` 前置）。
/// 跟 `geo.rs::node_program` 同口径 —— 那条路已经在客户机上验过了。
fn node_program() -> String {
    if let Some(dir) = crate::installer::portable_node_dir() {
        #[cfg(windows)]
        let exe = dir.join("node.exe");
        #[cfg(not(windows))]
        let exe = dir.join("node");
        if exe.exists() {
            return exe.display().to_string();
        }
    }
    "node".into()
}

fn inject_path(c: &mut Command) {
    let sep = if cfg!(windows) { ";" } else { ":" };
    let dirs = crate::installer::search_paths(crate::installer::portable_node_dir().as_deref());
    if dirs.is_empty() {
        return;
    }
    let prefix = dirs.iter().map(|d| d.display().to_string()).collect::<Vec<_>>().join(sep);
    let old = std::env::var("PATH").unwrap_or_default();
    c.env("PATH", format!("{prefix}{sep}{old}"));
}

/// 跑一条脚本，把它 stdout 上那行 JSON 拿回来。
///
/// **stdout 只收结果、stderr 全丢给错误信息** —— 这批脚本的 `--json` 契约就是这么写的
/// （见各脚本头部注释）。谁哪天往 stdout 里多打一行日志，这里会当场 parse 失败并把
/// 原文带回去，而不是静默返回一个空对象。
fn run_json(mut c: Command, what: &str, timeout_secs: u64) -> Result<Value, String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    inject_path(&mut c);
    c.stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(Stdio::null());
    let mut child = c.spawn().map_err(|e| format!("{what} 起不来: {e}"))?;
    // 死线靠调用方声明的 timeout_ms 兜底，这里再挡一层：脚本卡住不该把动作核心一起挂住。
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if start.elapsed().as_secs() > timeout_secs {
                    let _ = child.kill();
                    return Err(format!("{what} 超时（{timeout_secs}s）"));
                }
                std::thread::sleep(std::time::Duration::from_millis(80));
            }
            Err(e) => return Err(format!("{what} 等待失败: {e}")),
        }
    }
    let out = child.wait_with_output().map_err(|e| format!("{what} 读输出失败: {e}"))?;
    let so = String::from_utf8_lossy(&out.stdout);
    let se = String::from_utf8_lossy(&out.stderr);
    // **不能只看「最后一行以 { 开头」**：node 那批脚本 `--json` 打的是紧凑单行，
    // 但 Python 那条（read-doc.py）打的是缩进过的多行 JSON —— 按行找只会捞到一个孤零零的 `{`，
    // 然后报「输出不是合法 JSON」，把一次成功的读取说成脚本坏了。
    // 从第一个 `{` 起用流式解析器读一个完整的值，多行/带尾随日志都吃得下。
    let Some(start) = so.find('{') else {
        return Err(format!("{what} 没有输出 JSON。stderr: {}", se.trim()));
    };
    serde_json::Deserializer::from_str(&so[start..])
        .into_iter::<Value>()
        .next()
        .transpose()
        .map_err(|e| format!("{what} 输出不是合法 JSON: {e}；stderr: {}", se.trim()))?
        .ok_or_else(|| format!("{what} 没有输出 JSON。stderr: {}", se.trim()))
}

/// ★ 办公能力**能不能用** —— 无入参只读动作，`conformance` 会自动覆盖它。
///
/// 按 readiness 约定回答的是「能不能用」不是「装没装」：技能同步下来了但便携 Node 没装，
/// `doc.edit` 照样一步都跑不了。这两件事必须在同一份输出里分开说清楚，
/// 否则远程排障看到 `installed:true` 就不会再往下查了（RTK 那个坑的翻版）。
pub fn inspect() -> Value {
    let node = node_program();
    let node_ok = node != "node" || which_ok(&node);
    let py = crate::installer::python_for_docs();
    let checks = [
        ("uking-office-edit", "edit-office.mjs"),
        ("uking-office-read", "read-doc.py"),
        ("uking-docx", "gen-docx.mjs"),
        ("uking-xlsx", "gen-xlsx.mjs"),
        ("uking-ppt", "gen-pptx.mjs"),
        // doc.create.cad 靠它 —— 缺了这条包，出图纸动作一步都跑不了。
        ("uking-cad", "gen-dxf.mjs"),
    ];
    let scripts: Vec<Value> = checks
        .iter()
        .map(|(pack, file)| {
            let p = script(pack, file);
            json!({ "pack": pack, "script": file, "present": p.exists(), "path": p.to_string_lossy() })
        })
        .collect();
    let missing: Vec<&str> = checks
        .iter()
        .filter(|(pack, file)| !script(pack, file).exists())
        .map(|(pack, _)| *pack)
        .collect();

    let mut blockers: Vec<String> = Vec::new();
    if !missing.is_empty() {
        blockers.push(format!(
            "这些技能包还没同步到 ~/.uking/skills：{} —— 开机后台线程会补，也可以跑 runtime.skillpack.install",
            missing.join(" / ")
        ));
    }
    if !node_ok {
        blockers.push("找不到 node：改文档/出文档的脚本全跑不了。装机向导里装便携 Node 即可".into());
    }
    if py.is_none() {
        blockers.push(
            "找不到可用的 Python（便携版和系统 python3 都没有）：doc.read 读 PDF/Word 那条路跑不了（改文档不受影响）"
                .into(),
        );
    }
    json!({
        "ready": blockers.is_empty(),
        "blockers": blockers,
        "node": node,
        "python": py.map(|p| p.to_string_lossy().to_string()),
        "scripts": scripts,
    })
}

#[cfg(windows)]
fn which_ok(_exe: &str) -> bool {
    // 便携版路径已经 exists() 过；裸 "node" 的情况留给真跑时报错——
    // 为了体检去起一次子进程不值当（这个动作声明了 5 秒死线）。
    true
}
#[cfg(not(windows))]
fn which_ok(_exe: &str) -> bool {
    true
}

/// 读一份客户已有的文档 → Markdown 正文（保住表格）。`keywords` 非空时只摘相关段落。
///
/// **为什么带 keywords**：整份 100 页的招标文件丢进上下文既贵又常常放不下。
/// 这个参数不是优化，是这条路能不能用的前提。
pub fn read(file: &str, keywords: Option<&str>, max_chars: Option<u64>) -> Result<Value, String> {
    let s = script("uking-office-read", "read-doc.py");
    if !s.exists() {
        return Err("uking-office-read 技能包没同步到 ~/.uking/skills，先跑 runtime.skillpack.install".into());
    }
    let py = crate::installer::python_for_docs()
        .ok_or("not_installed: 找不到可用的 Python（便携版和系统 python3 都没有）—— 读文档这条路要它")?;
    let mut c = Command::new(py);
    // 🔴 Windows 上 Python 子进程的 stderr 默认走系统代码页（GBK），而 run_json 按 UTF-8 读
    // —— 缺依赖的报错一过管道就成锟斤拷，连 ERR_RULES 的中文词条都匹配不上（pc-*** 实测）。
    // 强制子进程 I/O 全走 UTF-8：stdout 的 JSON 本来就是 UTF-8，stderr 现在也能对上词表。
    c.env("PYTHONIOENCODING", "utf-8");
    c.env("PYTHONUTF8", "1");
    c.arg(&s).arg(file).arg("--json");
    if let Some(k) = keywords.filter(|k| !k.trim().is_empty()) {
        c.arg("--keywords").arg(k);
    }
    if let Some(m) = max_chars.filter(|m| *m > 0) {
        c.arg("--max-chars").arg(m.to_string());
    }
    run_json(c, "读文档", 120)
}

/// 在客户**已有的** Word / PPT / Excel 上改文字，格式一个字节不动。
///
/// `replacements` 是 `旧=>新` 的清单。**故意不接受正则** —— 正则在客户的合同上跑错一次，
/// 代价是他把改坏的文件发给了甲方，而我们连他改了什么都不知道。
pub fn edit(
    file: &str,
    replacements: &[String],
    out: Option<&str>,
    all_parts: bool,
) -> Result<Value, String> {
    // **入参校验排在环境检查前面**：这条调用不管技能包装没装都是错的，
    // 报「技能包没同步」会把调用方支去装东西，装完再调还是同一个错。
    if replacements.is_empty() {
        return Err("invalid_input: replacements 不能为空 —— 一次不改任何东西的「改文档」是空转".into());
    }
    let s = script("uking-office-edit", "edit-office.mjs");
    if !s.exists() {
        return Err("uking-office-edit 技能包没同步到 ~/.uking/skills，先跑 runtime.skillpack.install".into());
    }
    let mut c = Command::new(node_program());
    c.arg(&s).arg(file);
    for r in replacements {
        c.arg("--replace").arg(r);
    }
    if let Some(o) = out.filter(|o| !o.trim().is_empty()) {
        c.arg("--out").arg(o);
    }
    if all_parts {
        c.arg("--all-parts");
    }
    c.arg("--json");
    run_json(c, "改文档", 180)
}

// ─────────────────────── 出文档（doc.create.*）───────────────────────
//
// 与 read/edit 同源：**脚本就是实现**，这里只给稳定 ID、入参校验和确认门禁。
// 四个 gen-*.mjs 都吃「一个 JSON spec（或 markdown/csv）+ 一个 out 路径」，生成
// 确定性产物 —— 所以登记成幂等的写：同入参 + 同 out → 同一份文件，重放安全。

/// 把内容写进一个带进程号 + 序号的临时文件（保留扩展名，脚本按扩展名判断格式）。
fn write_temp(fname: &str, content: &str) -> Result<String, String> {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "uking-{}-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed),
        fname
    ));
    std::fs::write(&path, content).map_err(|e| format!("写临时文件失败: {e}"))?;
    Ok(path.to_string_lossy().to_string())
}

/// 把 `spec` 序列化到临时 JSON，返回路径。schema 已把 `spec` 声明成 object 并强校验，
/// 这里不再重查类型——但显式回报「不是对象」比让脚本去猜一个字符串更诚实。
fn spec_to_temp(fname: &str, spec: &Value) -> Result<String, String> {
    let content =
        serde_json::to_string_pretty(spec).map_err(|e| format!("spec 序列化失败: {e}"))?;
    write_temp(fname, &content)
}

/// 跑一条 `gen-*.mjs`：先查技能包在盘上（读同步下来的那份，见 [`script`]），再拼参数。
fn run_gen(pack: &str, script_name: &str, args: &[String], what: &str, timeout_secs: u64) -> Result<Value, String> {
    let s = script(pack, script_name);
    if !s.exists() {
        return Err(format!("{pack} 技能包没同步到 ~/.uking/skills，先跑 runtime.skillpack.install"));
    }
    let mut c = Command::new(node_program());
    c.arg(&s);
    for a in args {
        c.arg(a);
    }
    run_json(c, what, timeout_secs)
}

/// `out` 非空检查 —— 四个 create 共用。
fn require_out(out: &str) -> Result<(), String> {
    if out.trim().is_empty() {
        return Err("invalid_input: out 不能是空字符串".into());
    }
    Ok(())
}

/// 出 Word：`spec`（doc.json 的 blocks）和 `markdown` 二选一，`out` 是 .docx 路径。
pub fn create_word(spec: Option<&Value>, markdown: Option<&str>, out: &str) -> Result<Value, String> {
    require_out(out)?;
    // 入参校验排在环境检查前面：不给内容就把调用方支去装技能包，装完还是同一个错。
    let (flag, temp) = match (spec, markdown) {
        (Some(_), Some(_)) => return Err("invalid_input: spec 和 markdown 只能给一个".into()),
        (None, None) => return Err("invalid_input: spec 和 markdown 至少给一个".into()),
        (Some(sp), None) => ("--in", spec_to_temp("doc.json", sp)?),
        (None, Some(md)) => {
            if md.trim().is_empty() {
                return Err("invalid_input: markdown 不能是空字符串".into());
            }
            ("--md", write_temp("doc.md", md)?)
        }
    };
    let args = vec![flag.to_string(), temp, "--out".to_string(), out.to_string(), "--json".to_string()];
    run_gen("uking-docx", "gen-docx.mjs", &args, "生成 Word", 120)
}

/// 出 Excel：`spec`（book.json 的 sheets）和 `csv` 二选一，`out` 是 .xlsx 路径。
pub fn create_sheet(spec: Option<&Value>, csv: Option<&str>, out: &str) -> Result<Value, String> {
    require_out(out)?;
    let (flag, temp) = match (spec, csv) {
        (Some(_), Some(_)) => return Err("invalid_input: spec 和 csv 只能给一个".into()),
        (None, None) => return Err("invalid_input: spec 和 csv 至少给一个".into()),
        (Some(sp), None) => ("--in", spec_to_temp("book.json", sp)?),
        (None, Some(c)) => {
            if c.trim().is_empty() {
                return Err("invalid_input: csv 不能是空字符串".into());
            }
            ("--csv", write_temp("data.csv", c)?)
        }
    };
    let args = vec![flag.to_string(), temp, "--out".to_string(), out.to_string(), "--json".to_string()];
    run_gen("uking-xlsx", "gen-xlsx.mjs", &args, "生成 Excel", 120)
}

/// 出 PPT：`spec` 是 deck.json（title/accent/slides），`out` 是 .pptx 路径。
/// 脚本会顺带出一份同源 `.预览.html`（返回里带 `html` 字段）。
pub fn create_slide(spec: &Value, out: &str) -> Result<Value, String> {
    require_out(out)?;
    let temp = spec_to_temp("deck.json", spec)?;
    let args = vec!["--in".to_string(), temp, "--out".to_string(), out.to_string(), "--json".to_string()];
    run_gen("uking-ppt", "gen-pptx.mjs", &args, "生成 PPT", 180)
}

/// 出 CAD 图纸：`spec` 是 {title/layers/entities}，`out` 是 .dxf 路径。
/// 脚本会顺带出一份预览 SVG（返回里带 `preview` 字段）。
pub fn create_cad(spec: &Value, out: &str) -> Result<Value, String> {
    require_out(out)?;
    let temp = spec_to_temp("drawing.json", spec)?;
    let args = vec!["--in".to_string(), temp, "--out".to_string(), out.to_string(), "--json".to_string()];
    run_gen("uking-cad", "gen-dxf.mjs", &args, "生成 CAD 图纸", 120)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `inspect` 的形状必须稳定 —— 它是 conformance 唯一能自动跑的那条办公动作。
    #[test]
    fn inspect_always_answers_ready_and_blockers() {
        let v = inspect();
        assert!(v["ready"].is_boolean(), "ready 必须是布尔，不能缺");
        assert!(v["blockers"].is_array(), "blockers 必须是数组（哪怕空）");
        assert!(v["scripts"].as_array().map(|a| !a.is_empty()).unwrap_or(false));
        // ready 和 blockers 必须自洽：说没准备好却给不出原因，等于让排障的人去猜
        let has = !v["blockers"].as_array().map(|a| a.is_empty()).unwrap_or(true);
        assert_eq!(v["ready"].as_bool(), Some(!has), "ready 和 blockers 对不上");
    }

    /// 空 replacements 必须当场拒 —— 不许起一个什么都不改的子进程假装成功。
    #[test]
    fn edit_rejects_an_empty_replacement_list() {
        let e = edit("x.docx", &[], None, false).unwrap_err();
        assert!(e.contains("invalid_input"), "错误没标成入参问题: {e}");
    }

    /// create 的入参校验必须在碰脚本之前拦住：不给内容 / 两个都给 / 空 out，
    /// 都该报 `invalid_input`，而不是起一个注定失败的子进程。
    #[test]
    fn create_word_rejects_when_no_content_given() {
        let e = create_word(None, None, "x.docx").unwrap_err();
        assert!(e.contains("invalid_input"), "错误没标成入参问题: {e}");
    }

    #[test]
    fn create_word_rejects_when_both_forms_given() {
        let e = create_word(Some(&json!({ "title": "x" })), Some("# hi"), "x.docx").unwrap_err();
        assert!(e.contains("invalid_input"), "错误没标成入参问题: {e}");
    }

    #[test]
    fn create_slide_rejects_empty_out() {
        let e = create_slide(&json!({ "title": "x" }), "").unwrap_err();
        assert!(e.contains("invalid_input"), "错误没标成入参问题: {e}");
    }

    #[test]
    fn create_cad_rejects_empty_out() {
        let e = create_cad(&json!({ "entities": [] }), "  ").unwrap_err();
        assert!(e.contains("invalid_input"), "错误没标成入参问题: {e}");
    }
}
