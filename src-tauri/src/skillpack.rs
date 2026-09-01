//! AI 技能包导出 —— 把内嵌的 SKILL.md + CLI 脚本写到磁盘，供各 AI 工具
//! （OpenClaw / ClawX / Hermes / Claude Code 等）CLI 调用作图/视频。
//!
//! **独立可插拔模块**：纯 std、自带 reveal，删它只动 lib.rs（去 mod + command + handler）。
//! 对外只依赖 `installer`（问它要 Hermes 落点 / pi 装没装）和 `ulog`，方向是「新模块 → 老公共助手」，
//! 不反向。`cleanup` 反过来依赖本模块的 [`all_skill_dirs_on_disk`] —— 目录表是本模块的事实，
//! 别让它在别处再存一份。`#[tauri::command]` 全在 lib.rs 转调
//! （本模块不碰 AppHandle）。技能包文件**不含任何人的 Key** —— 脚本运行时自己从
//! `~/.uking/device.json` 读，所以导出的文件夹可随意分发。

use std::path::{Path, PathBuf};

/// 一个技能包：文件夹名（= Claude/ClawX 的「技能名」）+ 内含文件清单。
struct Pack {
    name: &'static str,
    files: &'static [(&'static str, &'static str)],
}

/// AIGC 包：作图 / 视频。文件夹名 = `uking-aigc`。
const AIGC: Pack = Pack {
    name: "uking-aigc",
    files: &[
        ("SKILL.md", include_str!("../skills/aigc/SKILL.md")),
        ("README.md", include_str!("../skills/aigc/README.md")),
        // 🔴 SKILL.md 第 198 行让 AI「选型看 MODELS.md」、第 145 行让它「做漫剧首选 gen-reel.mjs」。
        // 这两个文件在 skills/aigc/ 里一直是齐的，但漏进 include 清单 —— 客户机上装出来的技能包
        // 说明书指向两个不存在的文件，AI 照着调必然 ENOENT。**说明书写了的文件必须在这张表里。**
        ("MODELS.md", include_str!("../skills/aigc/MODELS.md")),
        ("scripts/gen-image.mjs", include_str!("../skills/aigc/scripts/gen-image.mjs")),
        ("scripts/gen-video.mjs", include_str!("../skills/aigc/scripts/gen-video.mjs")),
        ("scripts/gen-reel.mjs", include_str!("../skills/aigc/scripts/gen-reel.mjs")),
        ("scripts/gen-batch.mjs", include_str!("../skills/aigc/scripts/gen-batch.mjs")),
        ("scripts/gen-stitch.mjs", include_str!("../skills/aigc/scripts/gen-stitch.mjs")),
        ("scripts/gen-tts.mjs", include_str!("../skills/aigc/scripts/gen-tts.mjs")),
        ("scripts/gen-bgm.mjs", include_str!("../skills/aigc/scripts/gen-bgm.mjs")),
        ("scripts/gen-asr.mjs", include_str!("../skills/aigc/scripts/gen-asr.mjs")),
        ("scripts/list-models.mjs", include_str!("../skills/aigc/scripts/list-models.mjs")),
    ],
};

// 🔴 AIGC 清单是客户机实际拿到的脚本，不是源目录的影子。每个 `gen-*.mjs` 都必须进这张表：
// 少一个，说明书/编排器在开发机能跑、客户机却会 ENOENT。`aigc_generator_manifest_is_complete`
// 直接比对源目录，作为这张 include 清单的构建闸门；新增生成器时它会红，不能只改脚本忘了导出。
#[cfg(test)]
const AIGC_GENERATORS: &[&str] = &[
    "scripts/gen-image.mjs",
    "scripts/gen-video.mjs",
    "scripts/gen-reel.mjs",
    "scripts/gen-batch.mjs",
    "scripts/gen-stitch.mjs",
    "scripts/gen-tts.mjs",
    "scripts/gen-bgm.mjs",
    "scripts/gen-asr.mjs",
];

/// 看图包：给「只会文字」的模型（DeepSeek 等）当眼睛 —— 图片 OCR + 图像理解 + 定位。
/// 默认走**国产 qwen3.7-flash**：`skills/vision/bench/` 那条跑道上它是唯一扛住长截图的
/// （2400×2908 拿 4/4，次名 44%、再次 13%）。**旧默认 MiniMax-M3 已撤** —— 它在宽截图上
/// 不是漏读而是**整页编造**（泛问三遍全 0/7，编出不存在的按钮和账号），confidently wrong 更危险。
/// 脚本自读 `~/.uking/device.json` 的 Key，**不改工具的对话模型**——省钱路由不动，只在遇图时调一下。
/// 文件夹名 = `uking-vision`。
const VISION: Pack = Pack {
    name: "uking-vision",
    files: &[
        ("SKILL.md", include_str!("../skills/vision/SKILL.md")),
        ("scripts/see-image.mjs", include_str!("../skills/vision/scripts/see-image.mjs")),
        // 读 PDF -> Markdown：数字 PDF 直取文字（快准免费）且**表格按行列还原**
        // （压平成一维文本流会让下游拿邻格的数当答案：稀疏表实测 50% → 100%）；
        // 扫描页才渲染成图走 qwen3-vl-flash OCR。PyMuPDF 单 wheel 依赖，缺了自动 pip 装。
        ("scripts/read-pdf.py", include_str!("../skills/vision/scripts/read-pdf.py")),
    ],
};

/// 多 AI 协同包：让本机各 AI（claude/codex/openclaw/hermes）互相调用分工。文件夹名 = `uking-teamwork`。
const TEAMWORK: Pack = Pack {
    name: "uking-teamwork",
    files: &[
        ("SKILL.md", include_str!("../skills/teamwork/SKILL.md")),
        ("README.md", include_str!("../skills/teamwork/README.md")),
        ("scripts/call-agent.mjs", include_str!("../skills/teamwork/scripts/call-agent.mjs")),
    ],
};

/// 网站建站包（单文件 HTML+Tailwind 建站法，纯方法论无脚本）。给「网站设计专家」用。文件夹名 = `uking-web`。
const WEB: Pack = Pack {
    name: "uking-web",
    files: &[("SKILL.md", include_str!("../skills/web/SKILL.md"))],
};

/// PPT/幻灯片包：`gen-pptx.mjs` 纯 std 出**真 .pptx**（零 npm 依赖，客户机便携 Node 直接跑）
/// + SKILL.md 教「先大纲后填充」+ HTML 预览兜底。给「PPT·文档专家」用。文件夹名 = `uking-ppt`。
const PPT: Pack = Pack {
    name: "uking-ppt",
    files: &[
        ("SKILL.md", include_str!("../skills/ppt/SKILL.md")),
        ("scripts/gen-pptx.mjs", include_str!("../skills/ppt/scripts/gen-pptx.mjs")),
    ],
};

/// Word 文档包：`gen-docx.mjs` 纯 std 出**真 .docx**（Markdown/结构化块→Word，零 npm 依赖）
/// + SKILL.md。给「文档专家」用。文件夹名 = `uking-docx`。
const DOCX: Pack = Pack {
    name: "uking-docx",
    files: &[
        ("SKILL.md", include_str!("../skills/docx/SKILL.md")),
        ("scripts/gen-docx.mjs", include_str!("../skills/docx/scripts/gen-docx.mjs")),
    ],
};

/// Excel 表格包：`gen-xlsx.mjs` 纯 std 出**真 .xlsx**（CSV/结构化数据→Excel,数字保数值可求和，
/// 零 npm 依赖）+ SKILL.md。给「数据表格专家」用。文件夹名 = `uking-xlsx`。
const XLSX: Pack = Pack {
    name: "uking-xlsx",
    files: &[
        ("SKILL.md", include_str!("../skills/xlsx/SKILL.md")),
        ("scripts/gen-xlsx.mjs", include_str!("../skills/xlsx/scripts/gen-xlsx.mjs")),
    ],
};

/// ★ 读文档包（2026-08-03 新增）：补上「能出不能进」这个缺口 —— 上面 DOCX/XLSX/PPT/WEB
/// 四个包全是**生成**，客户手上那份 Word/Excel/PDF 却一直读不了，而办公场景九成是
/// 「我有一份文件，帮我……」。`read-doc.py` 把文档转成 Markdown（markitdown 优先、pandoc 兜底），
/// 并且**默认按关键词只摘相关段落**：实测一份 356KB 招标文件整份转出来是 11.3 万 token，
/// 本地摘完只剩 7852（7%），而问题问的本来就是那几个点。文件夹名 = `uking-office-read`。
const OFFICE_READ: Pack = Pack {
    name: "uking-office-read",
    files: &[
        ("SKILL.md", include_str!("../skills/officeread/SKILL.md")),
        (
            "scripts/read-doc.py",
            include_str!("../skills/officeread/scripts/read-doc.py"),
        ),
    ],
};

/// 「帮我把这份合同里的甲方改一下」。补的是「读」和「从零生成」之间那一格 ——
/// 客户拿来的 Word 带着公司模板（页眉页脚 / 编号 / 样式），读出来再用 `uking-docx` 重新生成
/// 等于把模板全丢了，而且是**静默降级**，客户打开文件那一刻才发现。
///
/// 这里只改文字：未修改的部件**直接复制原始压缩字节**，样式/页眉页脚/图片是**字节级相同**。
/// 真实 Word 会把一句话拆进几十个 `<w:t>`（实测一段最多 103 个），所以按段落拼接后再匹配 ——
/// 直接在 XML 上做字符串替换基本搜不到。文件夹名 = `uking-office-edit`。
const OFFICE_EDIT: Pack = Pack {
    name: "uking-office-edit",
    files: &[
        ("SKILL.md", include_str!("../skills/officeedit/SKILL.md")),
        (
            "scripts/edit-office.mjs",
            include_str!("../skills/officeedit/scripts/edit-office.mjs"),
        ),
    ],
};

/// CAD 图纸包：`gen-dxf.mjs` 纯 std 出**真 .dxf**（R12/AC1009，AutoCAD/浩辰/中望/CAD看图王/LibreCAD
/// 都认）**外加一张同源预览 SVG** —— 客户机上装了 CAD 的是少数，只给 dxf 等于「做完了但看不见」，
/// 跟 PPT 出同源 `.预览.html` 是同一条道理。
///
/// 为什么不用 ezdxf：那是 Python 库，客户机只有便携 Node。DXF R12 是纯文本格式，手搓反而最稳。
/// 文件**一律 UTF-8 写**：早期为了「保持纯 ASCII」用 latin1 写盘，中文图层名当场被截成垃圾字节
/// ——文字实体转义得再干净，图层名一样会毁掉整个文件。文件夹名 = `uking-cad`。
const CAD: Pack = Pack {
    name: "uking-cad",
    files: &[
        ("SKILL.md", include_str!("../skills/cad/SKILL.md")),
        ("scripts/gen-dxf.mjs", include_str!("../skills/cad/scripts/gen-dxf.mjs")),
    ],
};

/// 邮件包：`gen-eml.mjs` 纯 std 出**真 .eml 草稿**（Outlook/Foxmail/邮箱大师双击即开，
/// 收件人/主题/正文/附件全部预填，中文头按 RFC2047 编码）。
///
/// ★ **只写草稿、不发信**，`sent` 永远是 false：代发邮件是不可撤回的对外动作，
/// 做错一次就是事故。发不发由客户在自己的客户端按下发送键决定。文件夹名 = `uking-mail`。
const MAIL: Pack = Pack {
    name: "uking-mail",
    files: &[
        ("SKILL.md", include_str!("../skills/mail/SKILL.md")),
        ("scripts/gen-eml.mjs", include_str!("../skills/mail/scripts/gen-eml.mjs")),
    ],
};

/// 开网页 / 读网页包。补的是办公链路上「AI 看不见互联网、也不会把做完的东西开给人看」这一格：
/// - `open-url.mjs`：默认浏览器开网址、默认程序开本地产物（Windows 的 `start` 是 cmd 内建、
///   第一个参数还会被当窗口标题吃掉，模型反复踩，封一层就不用它记）；
/// - `fetch-page.mjs`：抓页面→本地剥壳→Markdown 正文（一个 200KB 页面通常只剩 3~8%，
///   直接 `curl` 整页丢进上下文又贵又常常撑爆）。
/// 文件夹名 = `uking-browse`。
const BROWSE: Pack = Pack {
    name: "uking-browse",
    files: &[
        ("SKILL.md", include_str!("../skills/browse/SKILL.md")),
        ("scripts/fetch-page.mjs", include_str!("../skills/browse/scripts/fetch-page.mjs")),
        ("scripts/open-url.mjs", include_str!("../skills/browse/scripts/open-url.mjs")),
    ],
};

/// 导出 PDF 包：办公链路的**最终交付形态**（合同发出去、报价给甲方、报告存档）。
/// 走本机 LibreOffice 保版式 —— 自己手搓只会出「字都在但版式全错」的东西，
/// 而客户要 PDF 恰恰就是为了版式不变。
///
/// ★ **没装 LibreOffice 就诚实报装不了，绝不静默降级**成「抽文字重排一份 PDF」：
/// 那种产物客户打开才发现版式没了，比直接说做不到坏得多。文件夹名 = `uking-pdf`。
const PDF: Pack = Pack {
    name: "uking-pdf",
    files: &[
        ("SKILL.md", include_str!("../skills/pdf/SKILL.md")),
        ("scripts/to-pdf.mjs", include_str!("../skills/pdf/scripts/to-pdf.mjs")),
    ],
};

/// 工作台包 —— **全仓唯一一个不带脚本的技能包，这是故意的**。
///
/// 别的包都是「一段说明 + 几个能跑的脚本」；这个只有说明。因为它要干的事
/// （建目录、写说明书、拦住往桌面乱撒文件）**已经在动作核心里了**
/// （`runtime.workbench.scan` / `.inspect` / `.install`）。再给它一份脚本就是同一件事两份实现，
/// 而漂开的那次正好是出事那次（宪法第 8/13 条）。
///
/// 所以它的内容是**方法**：怎么只读盘点客户那个乱文件夹、该问哪 4 句、
/// manifest 怎么写才合格 —— 然后调动作去落盘。
///
/// 🔴 **闸门写在动作核心不写在这份 md 里**：提示词里写「别覆盖客户的文件」，
/// 模型有一天就是会覆盖。文件夹名 = `uking-workbench`。
const WORKBENCH: Pack = Pack {
    name: "uking-workbench",
    files: &[("SKILL.md", include_str!("../skills/workbench/SKILL.md"))],
};

/// 全部技能包。新增一个包 = 这里加一行 + 上面定义。`export_to`/`install_into_tools` 自动一起释放。
const PACKS: &[&Pack] = &[
    &WORKBENCH,
    &AIGC,
    &VISION,
    &TEAMWORK,
    &WEB,
    &PPT,
    &DOCX,
    &XLSX,
    &OFFICE_READ,
    &OFFICE_EDIT,
    &CAD,
    &MAIL,
    &BROWSE,
    &PDF,
];

/// 本 U-King 版本会装进各工具的技能包文件夹名（`uking-aigc` 等）。给 cleanup 的「安全卸载」
/// **精确匹配**用——只删这几个，绝不用 `uking-*` 泛匹配去误删同名的其它技能（如开发机上的
/// 这类技能名形如 `xxx-patrol` / `xxx-remote-install`，那不是本 app 装的）。
pub fn pack_names() -> Vec<&'static str> {
    PACKS.iter().map(|p| p.name).collect()
}

/// ★ 能力目录：`(技能名, 一句话能干什么)`，从每个包 **SKILL.md 的 frontmatter 现抽**。
///
/// 给「给 AI 的说明书」(llms.txt) 用。**编译而不是手抄**：加一个技能包 → 说明书自动多一条，
/// 不会出现「技能包发了三版、说明书还停在第一版」那种漂移（同 identity 从动作表编译的做法）。
///
/// 为什么必须有这段：说明书原本只列 55 个动作（切驱动 / 查环境 / 装工具），
/// 技能包只在 `skillpack.install` 的描述里被顺带提了一句 —— 于是**别家 AI 读完根本不知道
/// 这台机器能做 PPT / CAD / 邮件 / 读文档**。能力存在但不可发现，等于没有。
///
/// 解析只认最简单的 `--- ... name: / description: ... ---` 头，抽不到就跳过该包
/// （宁可少一条，也不要把正文的某一行当成描述塞进说明书）。
pub fn skill_catalog() -> Vec<(String, String)> {
    PACKS
        .iter()
        .filter_map(|p| {
            let md = p.files.iter().find(|(rel, _)| *rel == "SKILL.md")?.1;
            let mut name = None;
            let mut desc = None;
            // frontmatter 只在文件开头那一段；正文里出现的 `description:` 不算。
            for line in md.lines().skip(1).take_while(|l| l.trim() != "---") {
                if let Some(v) = line.strip_prefix("name:") {
                    name = Some(v.trim().to_string());
                } else if let Some(v) = line.strip_prefix("description:") {
                    desc = Some(v.trim().to_string());
                }
            }
            Some((name.unwrap_or_else(|| p.name.to_string()), desc?))
        })
        .collect()
}

/// 全部包的 `(文件夹名, 包内相对路径)` 清单。给无头验证（`--skillpack-test`）逐个断言用 ——
/// **加一个包就自动多一组断言**，不必再去 lib.rs 手改文件名列表（那份手抄清单停在 AIGC 那 5 个
/// 文件上很久了，后加的 PPT/DOCX/XLSX/office-read 一个都没被盖住）。
pub fn pack_manifest() -> Vec<(&'static str, &'static str)> {
    PACKS
        .iter()
        .flat_map(|p| p.files.iter().map(move |(rel, _)| (p.name, *rel)))
        .collect()
}

/// 用户家目录。
///
/// 🔴 **必须复用公共层那一份**（`installer::user_home_dir()`，它认 `UKING_TEST_HOME`）。
/// 这里原本自己读 `USERPROFILE` —— 只读时没人发觉，2026-08-18 加了 [`uninstall_pack`]
/// （**真删目录**）之后当场出事：一条本该跑在沙箱里的单测，把开发机上真实的
/// `~/.claude|.codex|.agents/skills/uking-workbench` 三份全删了。
///
/// 教训不是「测试写错了」，是**一个不认沙箱的家目录实现，在模块只读时是潜伏的，
/// 加第一个写操作时才引爆**。identity.rs:50 记着一模一样的一笔。
fn home_dir() -> PathBuf {
    crate::installer::user_home_dir()
}

fn uking_home() -> PathBuf {
    home_dir().join(".uking")
}

/// 同步账本文件名。放在包根目录下，记「上一次我们写进去的每个文件长什么样」。
/// 用点号开头：AI 工具认的是 `SKILL.md`，不会把它当技能。
const SYNC_LEDGER: &str = ".uking-sync";

/// 一次同步的结果。`preserved` 不为 0 就意味着**动了客户改过的文件**（已留底），
/// 必须进日志 —— 静默覆盖别人的修改正是本模块此前的 bug。
#[derive(Default, Debug, PartialEq, Eq)]
struct SyncStat {
    /// 新建，或我们上次写的版本升级了
    written: usize,
    /// 磁盘上内容与要写的**完全一致** → 一个字节都不动（连 mtime 都不刷）
    skipped: usize,
    /// 客户改过（或没有账本可对） → 先留底 `*.uking-bak` 再覆盖
    preserved: usize,
}

/// FNV-1a 64。只用来回答「这个文件跟上次一样吗」，**不是密码学用途**，
/// 所以不引 sha256（那在 device.rs，import 它会破坏本模块「不依赖其它模块」的可插拔性）。
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// 读同步账本：`相对路径<TAB>十六进制哈希` 每行一条。读不到就当空（首次同步）。
fn read_ledger(root: &Path) -> std::collections::HashMap<String, u64> {
    let mut m = std::collections::HashMap::new();
    if let Ok(s) = std::fs::read_to_string(root.join(SYNC_LEDGER)) {
        for line in s.lines() {
            if let Some((rel, hex)) = line.split_once('\t') {
                if let Ok(h) = u64::from_str_radix(hex.trim(), 16) {
                    m.insert(rel.to_string(), h);
                }
            }
        }
    }
    m
}

/// 把单个包写到 `<parent>/<pack.name>/`（自动建子目录），返回该包根目录绝对路径 + 同步统计。
///
/// 🔴 **绝不静默覆盖客户改过的文件**（宪法第 10 条）。此前这里是无条件 `fs::write`，
/// 于是开机同步会把用户（或我们自己在开发机上）对技能的修改**整个吃掉，且毫无痕迹**。
/// 现在按三档处理：
///   1. 磁盘内容 == 要写的内容 → **跳过**，连 mtime 都不刷（免得每次开机都把全部技能文件改一遍时间戳）；
///   2. 磁盘内容 == 账本里「上次我们写的」 → 是我们自己的旧版本，直接覆盖升级；
///   3. 其它（客户改过，或首次同步没有账本可对） → 先留底 `<文件>.uking-bak` 再覆盖。
/// 留底是**单份覆盖式**（同 providers.rs 的 `*.uking-bak` 惯例），不按时间戳堆积。
fn write_pack(pack: &Pack, parent: &Path) -> Result<(String, SyncStat), String> {
    let root = parent.join(pack.name);
    std::fs::create_dir_all(&root).map_err(|e| format!("创建技能包目录失败: {e}"))?;
    let ledger = read_ledger(&root);
    let mut stat = SyncStat::default();
    let mut lines = String::new();

    for (rel, content) in pack.files {
        let p = root.join(rel);
        if let Some(par) = p.parent() {
            let _ = std::fs::create_dir_all(par);
        }
        let new_h = fnv1a(content.as_bytes());
        match std::fs::read(&p) {
            Ok(cur) => {
                let cur_h = fnv1a(&cur);
                if cur_h == new_h {
                    stat.skipped += 1;
                } else {
                    if ledger.get(*rel) == Some(&cur_h) {
                        stat.written += 1; // 我们上次写的，客户没动过
                    } else {
                        // 客户改过 / 无账本可对：先留底，再覆盖
                        let bak = p.with_file_name(format!(
                            "{}.uking-bak",
                            p.file_name().and_then(|s| s.to_str()).unwrap_or("file")
                        ));
                        let _ = std::fs::write(&bak, &cur);
                        stat.preserved += 1;
                    }
                    std::fs::write(&p, content).map_err(|e| format!("写入 {rel} 失败: {e}"))?;
                }
            }
            Err(_) => {
                std::fs::write(&p, content).map_err(|e| format!("写入 {rel} 失败: {e}"))?;
                stat.written += 1;
            }
        }
        lines.push_str(&format!("{rel}\t{new_h:016x}\n"));
    }

    // 账本写在最后：中途失败就不更新，下次仍按「客户可能改过」保守处理
    let _ = std::fs::write(root.join(SYNC_LEDGER), lines);
    Ok((root.display().to_string(), stat))
}

/// 把**全部**技能包（AIGC + 多 AI 协同）释放到 `<dest>/`（`dest=None` 用 `~/.uking/skills/`）。
/// 返回 **AIGC 包**根目录绝对路径——前端 blob 依赖它拼 `scripts/` 路径，必须是 aigc 那个，别改。
/// 纯函数，不弹对话框（对话框/reveal 由 lib.rs 转调）。
pub fn export_to(dest: Option<&Path>) -> Result<String, String> {
    let parent = match dest {
        Some(d) => d.to_path_buf(),
        None => uking_home().join("skills"),
    };
    let mut aigc_root = String::new();
    let mut total = SyncStat::default();
    for pack in PACKS {
        let (r, stat) = write_pack(pack, &parent)?;
        if stat.preserved > 0 {
            // 动了客户改过的文件，必须留痕：不写日志的话，客户回头问「我改的东西哪去了」查无对证
            crate::ulog::write(
                "skillpack",
                &format!("{} 有 {} 个文件被改过，已留底 *.uking-bak 再覆盖", pack.name, stat.preserved),
            );
        }
        total.written += stat.written;
        total.skipped += stat.skipped;
        total.preserved += stat.preserved;
        if pack.name == AIGC.name {
            aigc_root = r;
        }
    }
    crate::ulog::write(
        "skillpack",
        &format!(
            "同步完成：新写 {} · 未变跳过 {} · 客户改过已留底 {}",
            total.written, total.skipped, total.preserved
        ),
    );
    Ok(aigc_root)
}

/// 一个投放点：技能包该拷到哪个工具的哪个目录。
pub struct SkillTarget {
    /// 展示给客户看的工具名。
    pub label: &'static str,
    /// skills 父目录 —— 包会落在 `<parent>/<包名>/`。
    pub parent: PathBuf,
    /// 该工具在本机装了没。装的时候据此跳过（不给没装的工具留垃圾目录）；
    /// **卸载扫描不看这个字段**。
    pub installed: bool,
}

/// 各 AI 工具的 skills 父目录 —— **装（`install_into_tools`）与卸（`cleanup::skill_dirs`）
/// 共用的唯一一张表**。要加/改落点**只许改这里**。
///
/// 🔴 **它为什么长这样**：此前装和卸各自硬编码一份，早就漂了 —— 卸载那份只认
/// `~/.claude` + `~/.openclaw`，装进 `~/.codex/skills`、`~/.agents/skills` 的包**扫不到也删不掉**；
/// Hermes 那条扫的还是 `%LOCALAPPDATA%\hermes\skills\aigc`，而下面 Hermes 那段早写明那是**错落点**，
/// 于是「安全卸载」永远扫的是个空目录，真那份原封不动留在盘上。宪法第 8 条：
/// **同一事实存在几份就会漂移几份。**
///
/// `installed` 用「工具装没装」而不是「skills 父目录存不存在」：那个目录本来就该由第一个
/// 往里放东西的人建，等它自己出现 = 永远不装。
/// 某个技能包**当前装在哪几个工具里**（返回工具展示名）。
///
/// 🔴 **建立在 `skill_targets()` 这张唯一的落点表上**，不另起一份路径解析。
/// 这里原本是分支上的 `installed_in()`，它调的是重构前的 `skill_dirs()`；
/// main 后来把装/卸统一到 `skill_targets()`（理由见那个函数的注释：
/// 装和卸各自硬编码一份，早就漂了）。合并时把它按新形状重写，
/// **而不是把老函数塞回来** —— 塞回来就等于又造了第二张落点表。
///
/// ★ 不看 `installed`（那说的是「工具装没装」）：这里问的是
/// 「**包在不在盘上**」，卸载扫描同理，见 `pack_dirs_on_disk`。
pub fn installed_in(pack: &str) -> Vec<String> {
    skill_targets()
        .into_iter()
        .filter(|t| t.parent.join(pack).join("SKILL.md").is_file())
        .map(|t| t.label.to_string())
        .collect()
}

pub fn skill_targets() -> Vec<SkillTarget> {
    let home = home_dir();
    // Codex：`~/.codex/skills/<name>`（扁平，同 Claude）。依据不是我们猜的 —— Codex 自带的
    // 系统技能 `.system/skill-installer/SKILL.md` 里白纸黑字写着「Installs into
    // `$CODEX_HOME/skills/<skill-name>` (defaults to `~/.codex/skills`)」，且 `.system/` 下有
    // 它自己放的 `.codex-system-skills.marker`。所以认 `CODEX_HOME`，没设才回落 `~/.codex`。
    let codex = std::env::var("CODEX_HOME")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".codex"));
    // Hermes：认 `skills/<分类>/<名字>/` 的嵌套约定，`hermes skills list` 能把我们释放进去的包
    // 全列出来（`enabled`、分类 `aigc`），**不需要补 `metadata.hermes.tags`**
    // （对照组：Hermes 自带的 hyperframes 也没有该字段，同样被认）。
    //
    // 🔴 **落点必须问 Hermes 自己要，别自己猜**（pc-***，2026-08-05）：这里原先写着
    // 「`HERMES_HOME` 优先，没设才回落 `%LOCALAPPDATA%\hermes`」，还注明「等价于它
    // `hermes_constants.get_hermes_home()` 的解析顺序」—— **那句是错的**，它的真身只有两档：
    // `HERMES_HOME` → `~/.hermes`，压根没有 LOCALAPPDATA。于是 13 个包全释放进了 Hermes
    // 永远不扫的安装目录，`~/.hermes/skills` 下一个 `uking-*` 都没有。当初「实测全被认」
    // 也是在那个错落点上验的 —— 开发机 shell 里的 `HERMES_HOME` 残留（`Y:\compare-upstream`）
    // 让两个错误互相抵消了一次。判据统一收口到 `installer::hermes_config_dir()`。
    //
    // 全部包落在 `skills/aigc/` 这一个分类下。分类名对 uking-ppt / uking-cad 这些其实不贴切，
    // 但**别顺手改** —— 改了等于把客户机上已释放的那份变成孤儿副本，收益只是分类好看一点。
    let hermes = crate::installer::hermes_config_dir();
    vec![
        // Claude Code：~/.claude/skills/uking-aigc（官方 skills 目录，已验证）
        SkillTarget {
            label: "Claude Code",
            parent: home.join(".claude").join("skills"),
            installed: home.join(".claude").is_dir(),
        },
        SkillTarget {
            label: "Codex",
            installed: codex.is_dir(),
            parent: codex.join("skills"),
        },
        // OpenClaw / ClawX：agent 从 ~/.openclaw/skills 自动发现（已验证）
        SkillTarget {
            label: "OpenClaw/ClawX",
            parent: home.join(".openclaw").join("skills"),
            installed: home.join(".openclaw").is_dir(),
        },
        // ★ `~/.agents/skills/` —— **跨 harness 的 Agent Skills 标准目录**（agentskills.io/specification），
        // 不是某一家的私有路径。pi 明确实现了这个标准并从这里加载（包内 docs/skills.md 写着，
        // 2026-08-03 本机实测过：往里放一个带虚构税号的技能，`pi -p` 问报销问题，它一字不差答出来了）。
        // 其它遵循同一标准的 agent 顺带受益，一份多投，不用我们逐家适配。
        SkillTarget {
            label: "pi（Agent Skills 标准目录）",
            parent: home.join(".agents").join("skills"),
            installed: home.join(".pi").is_dir() || crate::installer::tool_installed("pi"),
        },
        SkillTarget {
            label: "Hermes",
            installed: hermes.is_dir(),
            parent: hermes.join("skills").join("aigc"),
        },
    ]
}

/// **只卸不装**的历史错落点：老版本 U-King 把 13 个包释放到过
/// `%LOCALAPPDATA%\hermes\skills\aigc`（Hermes 的**安装目录**，它运行时从不读）。
/// 落点已在 pc-*** 修掉，但**存量客户机上那份孤儿副本还躺着** —— 卸载得扫得到它，
/// 否则「安全卸载」报干净而盘上有残留。新代码永远不该往这里写。
fn legacy_skill_parents() -> Vec<PathBuf> {
    let mut v = Vec::new();
    #[cfg(windows)]
    {
        // 🔴 **沙箱优先**。这里原本直接读 `LOCALAPPDATA` —— 而 `UKING_TEST_HOME` 不重定向它，
        // 于是一次「沙箱内」的卸载测试真删掉了开发机上 `%LOCALAPPDATA%\hermes\skills\aigc\uking-ppt`
        // （2026-08-18 实测，本文件同一天已经因为 `home_dir()` 犯过一次同样的错）。
        //
        // 教训升级版：**认沙箱不能只认一个 env**。一个函数改对了不代表这个文件安全 ——
        // 只要还有第二处直接读环境变量拼路径，删除操作就还能逃出去。
        let local = match std::env::var("UKING_TEST_HOME") {
            Ok(t) if !t.trim().is_empty() => PathBuf::from(t).join("AppData").join("Local"),
            _ => match std::env::var("LOCALAPPDATA") {
                Ok(la) => PathBuf::from(la),
                Err(_) => return v,
            },
        };
        v.push(local.join("hermes").join("skills").join("aigc"));
    }
    v
}

/// 纯组合：投放点 + 历史错落点 → 卸载要扫的父目录。
/// 🔴 **不许按 `installed` 过滤** —— 工具被卸了、我们的包还躺在盘上，正是最需要扫到的情况。
///
/// 之所以把它拆成一个吃参数的纯函数，是为了能**脱离本机环境**测：开发机上五个工具全装着，
/// `installed` 恒为 true，直接测 `scan_parents()` 时「加个 `.filter(|t| t.installed)`」
/// 这个改法一点不变红（2026-08-11 变异验证实测过，确实不红）。
fn combine_scan_parents(targets: Vec<SkillTarget>, legacy: Vec<PathBuf>) -> Vec<PathBuf> {
    targets.into_iter().map(|t| t.parent).chain(legacy).collect()
}

/// 卸载扫描要覆盖的**全部**父目录。由 `uninstall_scan_covers_every_install_target` 用例守着。
fn scan_parents() -> Vec<PathBuf> {
    combine_scan_parents(skill_targets(), legacy_skill_parents())
}

/// 本 app 在本机**可能留下过**技能包的全部目录（当前落点 + 历史错落点），供卸载扫描。
pub fn all_skill_dirs_on_disk() -> Vec<PathBuf> {
    let mut v = Vec::new();
    for parent in scan_parents() {
        for name in pack_names() {
            let d = parent.join(name);
            if d.is_dir() {
                v.push(d);
            }
        }
    }
    v
}

/// **某一个**技能包当前在磁盘上的全部副本（各工具落点 + 历史错落点）。
///
/// 口径跟 [`all_skill_dirs_on_disk`] 完全一样，只是筛一个名字 —— 不另写一套扫描
/// （宪法第 8 条：同一事实存在几份就会漂移几份，这个模块为此已经翻过一次车）。
pub fn pack_dirs_on_disk(name: &str) -> Vec<PathBuf> {
    if !pack_names().contains(&name) {
        return Vec::new();
    }
    scan_parents()
        .into_iter()
        .map(|p| p.join(name))
        .filter(|d| d.is_dir())
        .collect()
}

/// 卸载**某一个**技能包：删掉它在各工具下的全部副本。
///
/// 🔴 为什么需要按包卸载（2026-08-18 客户反馈：「安装了太多预制 skill，还无法删除」）：
/// 在此之前本模块**只有装、没有拆** —— 16 个包一次性铺进客户的 `~/.claude/skills` 等落点，
/// 影核动作表里跟 skill 相关的只有 `runtime.skillpack.install` 一条。
/// 「安全卸载」页那条 `skills-in-tools` 是**全删**，粒度太粗：客户想辞掉一个专家，
/// 不该被迫把所有技能一起清掉。
///
/// **只删我们自己铺的包**（`pack_names()` 白名单，未知名字直接返回 0），
/// 绝不按 `uking-*` 泛匹配 —— 客户或第三方自己放的同前缀目录不归我们删。
pub fn uninstall_pack(name: &str) -> Result<Vec<String>, String> {
    if !pack_names().contains(&name) {
        return Err(format!("不认识这个技能包：{name}（只能卸载 U-King 自带的那几个）"));
    }
    let mut removed = Vec::new();
    for d in pack_dirs_on_disk(name) {
        match std::fs::remove_dir_all(&d) {
            Ok(()) => removed.push(d.display().to_string()),
            // 单个失败不挡其它落点（同 install 的 best-effort 纪律），但要记下来
            Err(e) => crate::ulog::write("skillpack", &format!("删 {} 失败: {e}", d.display())),
        }
    }
    Ok(removed)
}

/// **开箱默认铺的技能包**（其余的用户在「AI 专家」页那张清单上一键装）。
///
/// 🔴 为什么从 16 个减到 4 个（客户 2026-08-18：「有人抱怨我们安装了太多预制 skill」）：
/// 16 个包一次性铺进 `~/.claude/skills` 等 4~6 个落点 = 客户机上凭空多出五六十个目录，
/// 而它们**每一个的 SKILL.md 都会进 AI 的上下文**（同一天记过的 context-bloat 那条：
/// 43 个是别人铺的、我们又铺一批，两边叠起来把上下文撑爆）。
///
/// 留这 4 个的判据是「**不装就有明显缺口**」：
///  - `uking-aigc`        作图/视频 —— 产品的招牌能力，不装等于卖点没了
///  - `uking-vision`      看图/读 PDF —— DeepSeek 这类纯文本模型的眼睛，缺了会**编答案**
///  - `uking-office-read` 读客户拿来的 Word/Excel/PDF —— 办公场景九成是「我有一份文件…」
///  - `uking-workbench`   搭工作台 —— 一次性动作，且让后面每件事都更好用
///
/// 其余 12 个（ppt/docx/xlsx/cad/mail/browse/pdf/web/teamwork/office-edit/wechatmp/experts）
/// 都是**要用时才用得上**的产出类技能，清单上一键就能装回来。
///
/// 🔴 老客户升级后不会变少 —— 本函数只管「装」，不删已有的。
const DEFAULT_PACKS: &[&str] = &["uking-aigc", "uking-vision", "uking-office-read", "uking-workbench"];

/// 把技能包拷进**已装**的 AI 工具各自的 skills 目录。
/// 这样客户「只点了复制说明、没手动拷文件夹」也能让 AI 直接发现脚本（治路径发现痛点）。
/// best-effort：某个失败不影响其它。目录表见 [`skill_targets`]。
/// 返回 (工具名, 安装路径, experimental) 成功列表。`experimental=true` 表示「文件拷进去了，
/// 但尚未真机验证该工具是否认这个 skills 目录/SKILL.md 机制」——前端据此如实标注，不对客户假承诺。
pub fn install_into_tools() -> Vec<(String, String, bool)> {
    let mut done = Vec::new();
    crate::ulog::section("skillpack", "把 AI 技能包装进已装的工具");
    for t in skill_targets() {
        if !t.installed {
            continue;
        }
        // 成败都记：装进去了才是真装了。此前这里**失败是静默吞掉的**（`if let Ok` 没有 else），
        // 客户说「AI 找不到技能包」时根本查不到是没装还是装错地方。
        // 只铺 DEFAULT_PACKS —— 全量同步请走 `export_to`（那是 ~/.uking/skills 的源目录），
        // 或让用户在清单上按需装。**这里少铺 12 个，就是客户上下文里少 12 份 SKILL.md。**
        let mut ok = 0usize;
        for name in DEFAULT_PACKS {
            let Some(pack) = PACKS.iter().find(|p| p.name == *name) else { continue };
            match write_pack(pack, &t.parent) {
                Ok((p, _)) => {
                    crate::ulog::write("skillpack", &format!("{} ✓ {name} → {p}", t.label));
                    if ok == 0 {
                        done.push((t.label.to_string(), p, false));
                    }
                    ok += 1;
                }
                Err(e) => crate::ulog::write("skillpack", &format!("{} ✗ {name}: {e}", t.label)),
            }
        }
        if ok == 0 {
            crate::ulog::write("skillpack", &format!("{} ✗ 一个默认包都没铺成", t.label));
        }
    }
    done
}

/// 装**某一个**技能包进各已装工具（`uninstall_pack` 的反面）。
///
/// 🔴 有了按包卸载就必须有按包安装，否则删完想装回来只能把 16 个全铺一遍 ——
/// 那等于「用户自己定」只定得了一半（用户 2026-08-18：「能删能装就行，用户自己定」）。
///
/// 复用 `write_pack` / `skill_targets`：落点口径只此一份，别再写第二套。
pub fn install_pack(name: &str) -> Result<Vec<String>, String> {
    let pack = PACKS
        .iter()
        .find(|p| p.name == name)
        .ok_or_else(|| format!("不认识这个技能包：{name}"))?;
    let mut done = Vec::new();
    // 先落 ~/.uking/skills（脚本的规范位置，SKILL.md 里的命令都按它写），再拷进各工具
    for parent in std::iter::once(uking_home().join("skills"))
        .chain(skill_targets().into_iter().filter(|t| t.installed).map(|t| t.parent))
    {
        match write_pack(pack, &parent) {
            Ok((p, _)) => done.push(p),
            Err(e) => crate::ulog::write("skillpack", &format!("装 {name} 到 {} 失败: {e}", parent.display())),
        }
    }
    Ok(done)
}

/// 每个自带技能包当前的状态 —— 给「用户自己定装哪些」的界面用。
///
/// `installed` 的判据是**磁盘上真有那个目录**（`pack_dirs_on_disk` 非空），
/// 不是「我们调过 install」：调过不等于装成了（`install_into_tools` 是 best-effort，
/// 单个失败只记日志不报错），而客户看的是「现在到底有没有」。
pub fn pack_status() -> Vec<(String, String, bool, usize)> {
    let cat: std::collections::HashMap<String, String> = skill_catalog().into_iter().collect();
    PACKS
        .iter()
        .map(|p| {
            let dirs = pack_dirs_on_disk(p.name).len();
            (
                p.name.to_string(),
                cat.get(p.name).cloned().unwrap_or_default(),
                dirs > 0,
                dirs,
            )
        })
        .collect()
}

/// 在资源管理器(Windows)/Finder(macOS) 里打开目录。**自带极小实现，不抽 install.rs、不复用**
/// —— 保持本模块整块可插拔（对齐 video.rs 自带 base64/HTTP 包装的「叶子工具自包含」）。
/// explorer/open 是 GUI 进程、不弹黑窗，无需 CREATE_NO_WINDOW。best-effort，失败静默。
pub fn reveal_dir(path: &str) {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("explorer").arg(path).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(path).spawn();
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(path).spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 生成器必须同时存在于源目录和客户机导出清单。此处故意读目录而不是重抄文件名：
    /// 新增 `gen-foo.mjs` 却漏了 AIGC.files 时，测试立即失败，不能等客户机报 ENOENT。
    #[test]
    fn aigc_generator_manifest_is_complete() {
        let scripts = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("skills/aigc/scripts");
        let mut on_disk: Vec<String> = std::fs::read_dir(&scripts)
            .expect("AIGC scripts 目录应存在")
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| name.starts_with("gen-") && name.ends_with(".mjs"))
            .map(|name| format!("scripts/{name}"))
            .collect();
        on_disk.sort();
        let mut declared = AIGC_GENERATORS.iter().map(|s| (*s).to_string()).collect::<Vec<_>>();
        declared.sort();
        assert_eq!(declared, on_disk, "每个 gen-*.mjs 必须出现在 AIGC include 清单");

        let exported: Vec<&str> = AIGC.files.iter().map(|(path, _)| *path).collect();
        for generator in AIGC_GENERATORS {
            assert!(exported.contains(generator), "{generator} 未被 include_str! 导出给客户机");
        }
    }

    /// 落点必须**问目标工具自己要**，不许自己拼。
    ///
    /// 🔴 守的是 pc-*** 那次回归（2026-08-05 才查出来）：13 个包全释放进了
    /// `%LOCALAPPDATA%\hermes\skills\aigc` —— Hermes 的**安装目录**，它运行时从不读。
    /// 更坏的是当初「实测全被认」是在开发机 shell 的 `HERMES_HOME` 残留下验的，
    /// 两个错误互相抵消了一次，于是错误结论被当成验证通过写进了注释。
    ///
    /// 客户侧的表现是**静默失效**：包确实躺在盘上、卸载也扫得到，但
    /// `hermes skills list` 里一个 `uking-*` 都没有 —— 功能等于没装，没有任何报错。
    /// 这类 bug 只有断言「落点 == 目标工具自己的解析结果」才拦得住。
    ///
    /// 🔴 **必须进 `with_sandbox`，哪怕它不需要沙箱目录。** 这条用例读的 `HERMES_HOME` /
    /// `USERPROFILE` 都是**进程级** env，而 `cargo test` 默认多线程 —— 它自己不拿锁，
    /// 就会在别的模块正把 env 指向各自沙箱的那一瞬间读到别人的值，于是「单独跑全绿、
    /// 一起跑偶尔红」。2026-08-24 实测：连跑两次，第一次这条红、第二次 398 全绿，
    /// **失败信息里的「实际值」还长得完全合理**（就是另一个沙箱的路径），
    /// 最容易被当成偶发忽略掉。`testsandbox` 模块头的约定写得很清楚：
    /// 凡是碰进程级 env 的用例（**包括只读的**）一律走这把全进程唯一的锁。
    #[test]
    fn hermes_drop_point_follows_hermes_own_resolution() {
        crate::testsandbox::with_sandbox("skillpack-hermes-drop-point", &[], |_| {
            let targets = skill_targets();
            let h = targets.iter().find(|t| t.label == "Hermes").expect("必须有 Hermes 落点");
            assert_eq!(
                h.parent,
                crate::installer::hermes_config_dir().join("skills").join("aigc"),
                "Hermes 落点必须跟着 hermes_config_dir()（HERMES_HOME → ~/.hermes）走，别自己拼路径"
            );
        });
    }

    /// 历史错落点只许出现在「卸载要扫的」里，绝不许再回到「装进去的」里。
    /// 存量客户机上那份孤儿副本还躺着，扫不到就等于「安全卸载」报干净而盘上有残留。
    #[cfg(windows)]
    #[test]
    fn legacy_localappdata_is_uninstall_only() {
        let legacy = legacy_skill_parents();
        if legacy.is_empty() {
            return; // 没有 LOCALAPPDATA / 沙箱变量的环境，跳过（本条只在 Windows 有意义）
        }
        let targets = skill_targets();
        for p in &legacy {
            assert!(
                !targets.iter().any(|t| &t.parent == p),
                "历史错落点 {} 又回到安装目标里了 —— 它是 Hermes 运行时从不读的安装目录",
                p.display()
            );
        }
    }

    // 合成小包，别用真包：真包内容会随发版变，测试就跟着漂。
    const V1: Pack = Pack {
        name: "t-vision",
        files: &[("SKILL.md", "v1\n"), ("scripts/a.mjs", "aaa\n")],
    };
    // 只有 SKILL.md 变了 —— 模拟一次「我们发了新版」
    const V2: Pack = Pack {
        name: "t-vision",
        files: &[("SKILL.md", "v2\n"), ("scripts/a.mjs", "aaa\n")],
    };

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "uking-skillpack-test-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// 这条测试守的是本模块最贵的那个 bug：开机同步**静默吃掉**客户（或我们自己）
    /// 对技能的修改。2026-08-08 它真的发生过一次 —— 改好的看图技能被同步整个覆盖回旧版，
    /// 现场没有任何提示、没有留底。所以四种情形一次全断言，别拆散。
    #[test]
    fn sync_never_silently_clobbers_user_edits() {
        let parent = tmp("clobber");

        // ① 首次写入：两个文件都是新建
        let (root, s) = write_pack(&V1, &parent).unwrap();
        let root = PathBuf::from(root);
        assert_eq!(s, SyncStat { written: 2, skipped: 0, preserved: 0 }, "首次应全部新写");

        // ② 原样再同步一次：内容一致 → 一个字节都不该动
        let (_, s) = write_pack(&V1, &parent).unwrap();
        assert_eq!(s, SyncStat { written: 0, skipped: 2, preserved: 0 }, "没变就不该重写");

        // ③ 我们自己发新版：SKILL.md 升级、a.mjs 没变 → 直接覆盖，**不留底**
        let (_, s) = write_pack(&V2, &parent).unwrap();
        assert_eq!(s, SyncStat { written: 1, skipped: 1, preserved: 0 }, "我们写的旧版可直接升级");
        assert_eq!(std::fs::read_to_string(root.join("SKILL.md")).unwrap(), "v2\n");
        assert!(!root.join("SKILL.md.uking-bak").exists(), "我们自己的旧版不该产生留底");

        // ④ 客户动手改了 → 必须留底再覆盖，且留底里是**客户那份**
        std::fs::write(root.join("SKILL.md"), "客户自己加的一段\n").unwrap();
        let (_, s) = write_pack(&V2, &parent).unwrap();
        assert_eq!(s, SyncStat { written: 0, skipped: 1, preserved: 1 }, "客户改过的要计入 preserved");
        assert_eq!(
            std::fs::read_to_string(root.join("SKILL.md.uking-bak")).unwrap(),
            "客户自己加的一段\n",
            "留底必须是客户那份，不是我们的版本"
        );
        assert_eq!(std::fs::read_to_string(root.join("SKILL.md")).unwrap(), "v2\n");

        let _ = std::fs::remove_dir_all(&parent);
    }

    /// 老客户机上早就有技能文件、但没有账本 —— 这时**不能假设是我们写的**，
    /// 必须当客户改过来处理（保守留底）。首次升级会多出一批 .uking-bak，这是故意的。
    #[test]
    fn preexisting_files_without_ledger_are_treated_as_user_owned() {
        let parent = tmp("noledger");
        let root = parent.join(V1.name);
        std::fs::create_dir_all(root.join("scripts")).unwrap();
        std::fs::write(root.join("SKILL.md"), "来路不明的旧内容\n").unwrap();

        let (_, s) = write_pack(&V1, &parent).unwrap();
        assert_eq!(s.preserved, 1, "没有账本可对 → 保守当作客户的文件");
        assert_eq!(
            std::fs::read_to_string(root.join("SKILL.md.uking-bak")).unwrap(),
            "来路不明的旧内容\n"
        );
        let _ = std::fs::remove_dir_all(&parent);
    }

    /// 装和卸必须共用同一张目录表 —— 这条守的是「**卸载扫不到自己装的东西**」。
    ///
    /// 2026-08-11 之前 `cleanup.rs` 自己硬编码了一份，只认 `.claude` / `.openclaw` + 一个
    /// 已知的 Hermes 错落点。开发机实测：`~/.codex/skills` 14 个包、`~/.agents/skills` 14 个包
    /// **一个都扫不到**，「安全卸载」报干净而盘上留着 28 个目录。
    #[test]
    fn uninstall_scan_covers_every_install_target() {
        // ① 合成输入，故意让工具「没装」—— 这一段才是真正守住「按 installed 过滤」那个改法的。
        //    拿本机真实那张表测不出来：开发机上五个工具全装着，过滤掉的是空集。
        let scanned = combine_scan_parents(
            vec![
                SkillTarget { label: "装了的", parent: PathBuf::from("/x/a"), installed: true },
                SkillTarget { label: "没装的", parent: PathBuf::from("/x/b"), installed: false },
            ],
            vec![PathBuf::from("/x/legacy")],
        );
        assert!(scanned.contains(&PathBuf::from("/x/a")));
        assert!(
            scanned.contains(&PathBuf::from("/x/b")),
            "工具没装 ≠ 盘上没残留 —— 卸载扫描不许按 installed 过滤"
        );
        assert!(scanned.contains(&PathBuf::from("/x/legacy")), "历史错落点必须扫得到");

        // ② 真实那张表也过一遍：每个投放点都在卸载扫描范围内。
        //
        // 🔴 **必须进沙箱**（2026-08-11 修）：`scan_parents()` 和 `skill_targets()` 各自
        // 读一次环境（HOME / HERMES_HOME）来解析路径。裸跑时另一个测试会在这两次调用之间
        // 把 `UKING_TEST_HOME` 指到它自己的沙箱，于是两份快照来自不同的 home，断言当场炸：
        //
        //     投放点「Hermes」不在卸载扫描表里：
        //     …\Temp\uking-test-provider-list-per-tool-order\.hermes\skills\aigc
        //                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ 别的测试的沙箱名，铁证
        //
        // 实测 4 跑 2 红。**这不是「本机装了什么」的问题，是并行测试串了 env** ——
        // 走 `testsandbox` 这把全进程唯一的锁（模块头写着「一律走 with_sandbox，
        // 不要自己 set_var，也不要再起第二把锁」），env 在这一段里独占且稳定。
        //
        // 进沙箱不削弱这条断言：它守的是「装的每个点都在卸载扫描范围内」这个**不变量**，
        // 跟 home 具体指向哪儿无关；反倒因为不再受宿主环境影响而变得确定。
        crate::testsandbox::with_sandbox("skillpack-uninstall-scan", &[], |_| {
            let scanned = scan_parents();
            for t in skill_targets() {
                assert!(
                    scanned.contains(&t.parent),
                    "投放点「{}」不在卸载扫描表里：{}",
                    t.label,
                    t.parent.display()
                );
            }
            // ③ 历史错落点只卸不装 —— 扫得到，但永远不该再往里写
            for legacy in legacy_skill_parents() {
                assert!(
                    scanned.contains(&legacy),
                    "历史错落点 {} 必须仍被扫到，否则老客户机上的孤儿副本清不掉",
                    legacy.display()
                );
                assert!(
                    !skill_targets().iter().any(|t| t.parent == legacy),
                    "{} 是已知错落点（Hermes 运行时从不读的安装目录），不该出现在投放表里",
                    legacy.display()
                );
            }
            // ④ 表不能悄悄缩水 —— 返回 0 条也算绿是这类扫描的老盲区
            assert!(skill_targets().len() >= 5, "投放点数量不该缩水");
        });
    }

    #[test]
    fn ledger_roundtrips_and_ignores_garbage_lines() {
        let parent = tmp("ledger");
        let (root, _) = write_pack(&V1, &parent).unwrap();
        let root = PathBuf::from(root);
        let m = read_ledger(&root);
        assert_eq!(m.get("SKILL.md"), Some(&fnv1a(b"v1\n")));
        // 账本被写坏一行不该让整份作废（否则一个坏字节就把全部文件判成「客户改过」）
        std::fs::write(root.join(SYNC_LEDGER), "SKILL.md\tzzzz\n垃圾行\nscripts/a.mjs\t0000000000000001\n").unwrap();
        let m = read_ledger(&root);
        assert!(m.get("SKILL.md").is_none(), "坏哈希该被丢掉");
        assert_eq!(m.get("scripts/a.mjs"), Some(&1u64));
        let _ = std::fs::remove_dir_all(&parent);
    }


    /// 按包卸载：能真删、只删自己的、幂等。
    ///
    /// 🔴 这条守的是客户 2026-08-18 的原话「安装了太多预制 skill，还无法删除……要能删，真删」。
    /// 在此之前本模块**只有装、没有拆**，影核动作表里跟 skill 相关的只有 install 一条。
    ///
    /// 三件事一次断言，别拆散 —— 它们互为对方的安全网：
    ///  1. **真删**：磁盘上那个目录没了（不是标记成隐藏、不是只删 SKILL.md）
    ///  2. **只删自己的**：白名单之外的名字一律 Err，绝不按 `uking-*` 泛匹配 ——
    ///     客户或第三方自己放的同前缀目录不归我们删（这台开发机上 `~/.claude/skills`
    ///     58 个目录里就有 39 个不是我们的）
    ///  3. **幂等**：已经没了再调一次照样 Ok、removed 为空。契约里声明了幂等就得兑现，
    ///     否则重试会变成报错（`actions.rs` 那条「只登记 idempotent 的写」）
    #[test]
    fn uninstall_pack_removes_only_our_own_and_is_idempotent() {
        let sb = crate::testsandbox::enter("skillpack-uninstall", &[".claude/skills"]);
        let skills = sb.root().join(".claude").join("skills");

        // 拿一个真包名（白名单里的）建目录，外加一个**同前缀但不是我们的**
        let ours = pack_names()[0];
        let theirs = "uking-someone-elses";
        for n in [ours, theirs] {
            std::fs::create_dir_all(skills.join(n)).unwrap();
            std::fs::write(skills.join(n).join("SKILL.md"), "x").unwrap();
        }

        // ② 只删自己的：白名单外的名字必须被拒，且**目录原封不动**
        assert!(uninstall_pack(theirs).is_err(), "把不是我们的包也当自己的删了");
        assert!(skills.join(theirs).is_dir(), "拒绝了却还是把人家的目录删了");

        // ① 真删
        let removed = uninstall_pack(ours).expect("卸载自家包不该失败");
        assert!(!removed.is_empty(), "说卸载成功却一个目录都没删：{removed:?}");
        assert!(!skills.join(ours).exists(), "目录还在 —— 「删除」是假的");

        // ③ 幂等
        let again = uninstall_pack(ours).expect("重复卸载必须成功（契约声明了幂等）");
        assert!(again.is_empty(), "第二次卸载不该报删了东西：{again:?}");

        // 人家的还在
        assert!(skills.join(theirs).is_dir(), "顺手把不是我们的删了");
    }

    /// 开箱默认只铺 4 个，其余 12 个仍在表里、能按需装。
    ///
    /// 🔴 这条守的是客户 2026-08-18 那句「安装了太多预制 skill」。
    /// 判据不是「DEFAULT_PACKS 长度等于 4」——那只是把常量抄一遍，改错了照样绿；
    /// 而是**真跑一遍 install_into_tools 之后磁盘上有几个目录**。
    ///
    /// 反向那半边同样要钉：`pack_names()` 必须仍是全量，否则「默认少装」会变成
    /// 「其余的永远装不回来」——那比多装坏得多（客户会说"以前会做 PPT 现在不会了"）。
    #[test]
    fn default_install_is_a_small_subset_but_all_packs_stay_available() {
        let sb = crate::testsandbox::enter("skillpack-default", &[".claude"]);
        // 让 skill_targets 认为 Claude 装了：造出它的 skills 父目录的上级
        let claude = sb.root().join(".claude");
        std::fs::create_dir_all(claude.join("skills")).unwrap();

        for name in DEFAULT_PACKS {
            assert!(pack_names().contains(name), "默认包 {name} 不在总表里 —— 拼错了");
        }
        assert!(
            DEFAULT_PACKS.len() * 3 < pack_names().len(),
            "默认铺的占比太高了，等于没减（默认 {} / 全部 {}）",
            DEFAULT_PACKS.len(),
            pack_names().len()
        );

        // 真跑一遍，数磁盘
        let _ = install_into_tools();
        let on_disk: Vec<String> = pack_names()
            .into_iter()
            .filter(|n| !pack_dirs_on_disk(n).is_empty())
            .map(str::to_string)
            .collect();
        for name in DEFAULT_PACKS {
            assert!(on_disk.iter().any(|x| x == name), "默认包 {name} 没铺上：{on_disk:?}");
        }
        let extra: Vec<&String> = on_disk.iter().filter(|n| !DEFAULT_PACKS.contains(&n.as_str())).collect();
        assert!(extra.is_empty(), "开箱铺了不该铺的：{extra:?}");

        // 其余的仍装得回来（这半边不能丢）
        let optional = pack_names().into_iter().find(|n| !DEFAULT_PACKS.contains(n)).unwrap();
        assert!(!install_pack(optional).unwrap().is_empty(), "非默认包装不回来了：{optional}");
        assert!(!pack_dirs_on_disk(optional).is_empty(), "说装上了但磁盘上没有：{optional}");
    }
}
