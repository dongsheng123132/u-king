#!/usr/bin/env python3
# U-King 读 PDF -> Markdown —— 给「只会文字」的 AI 当 PDF 阅读器。
#
# 正确架构（业界共识，见虾盘云能力路线图）：**先抽文字，不是让模型直接看页图**。
#   - 数字 PDF（有内嵌文字，占绝大多数）: PyMuPDF 直接抽文字/**表格按行列还原** -> Markdown，**快、准、免费**。
#   - 扫描件/图片 PDF（抽不出文字）: 该页渲染成图 -> 虾盘云视觉模型 OCR -> Markdown（按页收费，仅扫描页走）。
# 输出 Markdown 到 stdout（每页 "## 第 N 页" 分隔），交给 agent 的对话模型消费。
#
# 依赖：PyMuPDF，单 wheel 无 GPU，缺了自动 pip 安装。Key 自读 ~/.uking/device.json（脚本不含 Key）。
# 用法：python read-pdf.py <pdf路径> [--max-pages N] [--ask "问题"] [--json] [--no-ocr] [--no-tables]

import sys, os, json, base64, subprocess, urllib.request, urllib.error

BASE = "https://api.u-claw.org.cn"          # 国内可达域（.org 子域被 GFW SNI 阻断）
# 扫描页 OCR 用。实测（96 行密集长图整页转录，数转出几行）：
#   qwen3-vl-flash 96/96 行 42s 3094tok  ← 选它：同样转全，比 qwen-vl-max 快 40%
#   qwen-vl-max    96/96 行 71s 2905tok
#   qwen3.5-ocr    95/96 行 38s 撞 token 上限被截断、页脚丢失（专用 OCR 模型反而最差）
# 按页收费，PDF 常有几十页，速度直接决定体验，故不用 max 档。
OCR_MODEL_DEFAULT = "qwen3-vl-flash"
TEXT_MIN = 40                               # 一页抽出的文字少于这个字符数 = 视作扫描页，转 OCR
OCR_DPI = 150                               # 渲染扫描页的分辨率（够 OCR、不至于太大）


def eprint(*a):
    print(*a, file=sys.stderr)


def parse_args(argv):
    out = {"_": [], "max_pages": 0, "ocr_model": OCR_MODEL_DEFAULT, "json": False,
           "no_ocr": False, "no_tables": False, "ask": ""}
    i = 0
    while i < len(argv):
        a = argv[i]
        if a == "--json":
            out["json"] = True
        elif a == "--no-ocr":
            out["no_ocr"] = True
        elif a == "--no-tables":
            out["no_tables"] = True
        elif a == "--ask" and i + 1 < len(argv):
            i += 1
            out["ask"] = argv[i]
        elif a == "--max-pages" and i + 1 < len(argv):
            i += 1
            out["max_pages"] = int(argv[i] or 0)
        elif a == "--ocr-model" and i + 1 < len(argv):
            i += 1
            out["ocr_model"] = argv[i]
        elif a == "--key" and i + 1 < len(argv):
            i += 1
            out["key"] = argv[i]
        elif not a.startswith("--"):
            out["_"].append(a)
        i += 1
    return out


def resolve_key(args):
    if args.get("key"):
        return str(args["key"])
    if os.environ.get("XIAPAN_API_KEY"):
        return os.environ["XIAPAN_API_KEY"]
    try:
        p = os.path.join(os.path.expanduser("~"), ".uking", "device.json")
        with open(p, "r", encoding="utf-8") as f:
            j = json.load(f)
        if j.get("key"):
            return j["key"]
    except Exception:
        pass
    return ""


def ensure_fitz():
    try:
        import fitz  # noqa
        return
    except Exception:
        pass
    eprint("首次使用，正在安装 PDF 引擎 PyMuPDF（单包、约 15MB、一次性）…")
    # 用当前 python 装，走 U-King 便携 Python 的 pip；prefer-binary 避免编译
    subprocess.run(
        [sys.executable, "-m", "pip", "install", "--quiet", "--disable-pip-version-check",
         "--prefer-binary", "pymupdf"],
        check=False,
    )


def page_text_with_tables(page):
    """一页 -> Markdown，**表格按行列还原**，其余段落按阅读顺序。

    为什么必须这么干：`page.get_text("text")` 把表格压成一维文本流 ——
    数字一个不少，但「这个数是哪一行哪一列的」全没了。**空单元格是杀手**：
    它在文本流里不留任何字符，于是「少了哪一列」无从判断，下游会拿邻格的数当答案。
    实测（doc-bench，下游 deepseek-v4-flash 真答题）稀疏表：压平 9/18，还原后 18/18。
    典型错法原话：问「丙项目的预算」答「丙项目的【预算】是 4,100。」——那是隔壁「已用」列。

    做法：**不自己排版** —— 保留 `get_text("text")` 的阅读顺序（它已经处理好了
    双栏、跨栏这些版面），只把属于表格的那几行**原位**换成 Markdown 表格。

    🔴 别改回「按 bbox 收块 + 按 y 排序」：那样表格是对了，但双栏正文会被按
    纵坐标打散成左右交叉串行（左栏一句、右栏一句……）。

    返回 (markdown, 表格数)。
    """
    full = page.get_text("text")
    try:
        tables = sorted(page.find_tables().tables, key=lambda t: t.bbox[1])
    except Exception:
        tables = []
    if not tables:
        return full.strip(), 0

    lines = full.splitlines()
    n_ok = 0
    for t in tables:
        try:
            md = t.to_markdown().strip()
            tlines = [l.strip() for l in page.get_text("text", clip=t.bbox).splitlines() if l.strip()]
        except Exception:
            continue
        if not tlines or not md:
            continue
        # 在正文行里找到这张表的起点，整段换成 Markdown（表格在文本流里是连续的）
        start = next((i for i, l in enumerate(lines) if l.strip() == tlines[0]), -1)
        if start < 0:
            continue
        end, ptr = start, 0
        while end < len(lines) and ptr < len(tlines):
            if lines[end].strip() == tlines[ptr]:
                ptr += 1
            elif lines[end].strip():
                break  # 撞上不属于这张表的内容，就此打住，宁可少换不要吃掉正文
            end += 1
        if ptr < len(tlines) * 0.8:
            continue  # 对不上八成以上，说明定位不可靠，放弃这张表、保留原文
        lines[start:end] = [md]
        n_ok += 1

    return "\n".join(lines).strip(), n_ok


def ocr_page_png(png_bytes, key, model, ask=""):
    """一页扫描图 -> 虾盘云视觉模型 OCR -> Markdown 文本。

    `ask` = focus hint。实测：泛泛地问，弱模型会整页编造；
    带上「你要找什么」，同一模型同一张图能从 0/7 提到 5.7/7。扫描页同理，
    **但它只加不减** —— 仍然要求整页转全，focus 只是让它别把关键区糊过去。
    """
    data_url = "data:image/png;base64," + base64.b64encode(png_bytes).decode("ascii")
    instruction = ("把这一页 PDF 里的内容**准确**转成 Markdown：正文按原文，表格转成 Markdown 表格，"
                   "公式转 LaTeX，保持标题层级。只输出内容本身，不要解释、不要加「以下是」这类话。")
    if ask:
        instruction += ("\n注意：下游要回答的问题是「%s」。整页仍需转全，"
                        "但与该问题相关的数字、字段、表格单元格务必逐字准确，不要概括。" % ask)
    body = json.dumps({
        "model": model,
        "messages": [{"role": "user", "content": [
            {"type": "text", "text": instruction},
            {"type": "image_url", "image_url": {"url": data_url}},
        ]}],
        "max_tokens": 4000,
        "temperature": 0,
    }).encode("utf-8")
    req = urllib.request.Request(
        BASE + "/v1/chat/completions", data=body, method="POST",
        headers={"Authorization": "Bearer " + key, "Content-Type": "application/json"},
    )
    try:
        with urllib.request.urlopen(req, timeout=120) as r:
            d = json.load(r)
        return d["choices"][0]["message"]["content"].strip()
    except urllib.error.HTTPError as e:
        return "（本页 OCR 失败：HTTP %s %s）" % (e.code, e.read().decode("utf-8", "ignore")[:120])
    except Exception as e:
        return "（本页 OCR 失败：%s）" % e


def main():
    # 🔴 stdout 只许出结果。PyMuPDF 会用**裸 print** 往 stdout 打提示
    # （`fitz` 弃用警告、`find_tables` 的 "Consider using pymupdf_layout…"），
    # 一行就能让 `--json | jq` 当场崩。逐条去堵堵不干净（下个版本还会加新的），
    # 所以整段处理期间把 sys.stdout 改道到 stderr，最后再换回来打结果。
    real_stdout = sys.stdout
    sys.stdout = sys.stderr

    args = parse_args(sys.argv[1:])
    src = args["_"][0] if args["_"] else ""
    if not src:
        eprint("用法：python read-pdf.py <pdf路径> [--max-pages N] [--no-ocr] [--json]")
        sys.exit(2)
    if not os.path.exists(src):
        eprint("读不到 PDF 文件：" + src)
        sys.exit(2)

    ensure_fitz()
    try:
        # 🔴 必须 import pymupdf 而不是老名字 fitz：老名字会把弃用警告打到 stdout。
        import pymupdf
    except Exception as e:
        eprint("PDF 引擎 PyMuPDF 安装失败：%s。请在 U-King「厨具工具箱」装 Python，或手动 pip install pymupdf。" % e)
        sys.exit(1)

    doc = pymupdf.open(src)
    total = doc.page_count
    limit = args["max_pages"] if args["max_pages"] > 0 else total
    key = resolve_key(args)

    pages_md, n_text, n_ocr, n_skip, n_tables = [], 0, 0, 0, 0
    for i in range(min(limit, total)):
        page = doc[i]
        if args["no_tables"]:
            text, ntab = page.get_text("text").strip(), 0
        else:
            text, ntab = page_text_with_tables(page)
        if len(text) >= TEXT_MIN:
            pages_md.append("## 第 %d 页\n\n%s" % (i + 1, text))
            n_text += 1
            n_tables += ntab
        elif args["no_ocr"]:
            pages_md.append("## 第 %d 页\n\n（扫描页，未开 OCR）" % (i + 1))
            n_skip += 1
        elif not key:
            pages_md.append("## 第 %d 页\n\n（扫描页需 OCR，但没找到虾盘云 Key，跳过）" % (i + 1))
            n_skip += 1
        else:
            eprint("第 %d 页是扫描件，转视觉模型 OCR 中…" % (i + 1))
            pix = page.get_pixmap(dpi=OCR_DPI)
            md = ocr_page_png(pix.tobytes("png"), key, args["ocr_model"], args["ask"])
            pages_md.append("## 第 %d 页\n\n%s" % (i + 1, md))
            n_ocr += 1

    markdown = "\n\n---\n\n".join(pages_md)
    sys.stdout = real_stdout  # 从这里开始才是真结果
    if args["json"]:
        print(json.dumps({
            "ok": True, "pages": min(limit, total), "total_pages": total,
            "text_pages": n_text, "ocr_pages": n_ocr, "skipped": n_skip,
            "tables": n_tables, "markdown": markdown,
        }, ensure_ascii=False))
    else:
        print(markdown)
    eprint("完成：%d 页（文字直取 %d · 扫描OCR %d · 跳过 %d，共 %d 页），还原表格 %d 个"
           % (min(limit, total), n_text, n_ocr, n_skip, total, n_tables))


if __name__ == "__main__":
    main()
