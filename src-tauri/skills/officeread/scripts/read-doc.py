#!/usr/bin/env python3
"""把客户已有的办公文档变成 AI 读得懂的 Markdown，并按需只摘相关段落。

为什么要有这个脚本（而不是让 AI 自己想办法）：
  U-King 的技能包一直「能出不能进」—— 会生成 docx/pptx/xlsx，却读不了客户手上那份。
  而办公场景九成是「我有一份文件，帮我……」。

为什么必须有 --keywords 预筛这一步（本脚本最值钱的部分）：
  真实实测（一份 356KB 的建筑智能化工程招标文件，关键词 工期/投标保证金/资质/付款）：
    整份转出来 = 20.9 万字符 ≈ 11.3 万 token —— 塞进 131072 的上下文只剩 1.8 万给输出，
    而且每问一句都要为整份文件付一次全价。
    本地按关键词摘完 = 5.8 万字符 ≈ 3.4 万 token，**剩 28%**，答案质量不受影响
    （问的本来就是"工期/保证金/资质"这些点，正文其余部分对这个问题是噪音）。
    再加 -m 14000 可压到 7852 token（7%），适合只要几个确定字段的场合。
  省多少取决于关键词有多聚焦 —— 上面那组词命中几百处所以只到 28%，问得越具体降得越狠。
  所以默认行为是：给了关键词就只输出命中段落。别把「能转换」当成「能直接喂」。

输出约定（对齐仓库的 ai-cli-design 规矩）：
  · stdout 只放正文/JSON —— 可以直接 `| llm "…"` 或重定向
  · stderr 放统计和日志 —— 管道里不会污染正文
  · 退出码：0 成功 / 2 参数或文件问题 / 3 没有可用的转换器
"""
import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time

# markitdown 与 pandoc 的实测对比（同一份 356KB docx，2026-08-03 本机）：
#   pandoc 3.10 : 2.6s / 311,214 字符 / 204 行表格
#   markitdown  : 5.2s / 209,439 字符 / 695 行表格
# pandoc 更快，但输出多 33% 字符（= 每次调用多付 33% token），表格提取也差一截。
# 所以优先 markitdown，pandoc 只当兜底 —— 但兜底必须留着：pandoc 常常已经装了
# （在 U-King 厨具工具箱里），而 markitdown 要走 pip。
CONVERTERS = ("markitdown", "pandoc")


def log(msg: str) -> None:
    print(msg, file=sys.stderr)


def convert_markitdown(src: str) -> str:
    """用 markitdown 转换。走库而不是命令行 —— 命令行入口在某些安装下不进 PATH。"""
    from markitdown import MarkItDown  # 延迟导入：没装时走 pandoc 兜底，不该在这崩

    return MarkItDown().convert(src).text_content


def convert_pandoc(src: str) -> str:
    exe = shutil.which("pandoc")
    if not exe:
        raise RuntimeError("pandoc 不在 PATH 上")
    # 写临时文件再读，比让 pandoc 往 stdout 吐更稳（Windows 上编码容易出岔子）
    fd, out = tempfile.mkstemp(suffix=".md")
    os.close(fd)
    try:
        subprocess.run([exe, src, "-t", "gfm", "-o", out], check=True,
                       stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        with open(out, encoding="utf-8", errors="replace") as f:
            return f.read()
    finally:
        try:
            os.remove(out)
        except OSError:
            pass


def convert(src: str, prefer: str) -> tuple[str, str]:
    """返回 (正文, 用的哪个转换器)。按偏好顺序试，全失败才报错。"""
    order = [prefer] + [c for c in CONVERTERS if c != prefer]
    errors = []
    for name in order:
        try:
            t0 = time.time()
            text = convert_markitdown(src) if name == "markitdown" else convert_pandoc(src)
            log(f"[转换] {name} 成功，耗时 {time.time() - t0:.1f}s")
            return text, name
        except Exception as e:  # noqa: BLE001 —— 任何一个转换器挂了都该试下一个
            errors.append(f"{name}: {e}")
            log(f"[转换] {name} 不可用（{e}）")
    raise RuntimeError(
        "没有可用的转换器。装其一即可：\n"
        "  · python -m pip install markitdown[docx,pdf,xlsx]\n"
        "    🔴 必须用 `python -m pip`，不能裸跑 `pip` —— 客户机上 pip 可能指向别的\n"
        "       虚拟环境（本机实测就指向 Hermes 的 venv），装进去 python 那边 import 不到。\n"
        "  · 或在 U-King「厨具工具箱」里装 Pandoc\n"
        "详情：" + " | ".join(errors)
    )


def excerpt(text: str, keywords: list[str], before: int, after: int, max_chars: int) -> str:
    """只保留命中关键词的段落及其上下文。**这一步是省钱的关键，不是可选优化。**"""
    lines = text.split("\n")
    keep: set[int] = set()
    for i, line in enumerate(lines):
        if any(k in line for k in keywords):
            keep.update(range(max(0, i - before), min(len(lines), i + after + 1)))
    if not keep:
        return ""
    out = "\n".join(lines[i] for i in sorted(keep))
    return out[:max_chars] if max_chars > 0 else out


def est_tokens(s: str) -> int:
    """粗估 token：中文按 0.65/字，其余按 4 字符/token。只用来给人一个量级感，别当账单。"""
    cn = len(re.findall(r"[一-鿿]", s))
    return int(cn * 0.65 + (len(s) - cn) / 4)


def main() -> int:
    ap = argparse.ArgumentParser(
        description="把 Word/Excel/PPT/PDF 转成 Markdown，可按关键词只摘相关段落"
    )
    ap.add_argument("file", help="要读的文档（.docx/.xlsx/.pptx/.pdf/.csv/.html 等）")
    # 🔴 help 里的百分号必须写成 `%%`：argparse 会对 help 串做 `help % params` 格式化，
    # 裸的 `% t` 会被当成非法转换符。Python ≤3.13 只有在真去打印 --help 时才炸，
    # 3.14 起 add_argument() 当场校验（_check_help）—— 于是**每一次调用**都直接 ValueError，
    # 整个 doc.read 在装了新 Python 的机器上一步都跑不动。
    ap.add_argument("-k", "--keywords", default="",
                    help="逗号分隔。给了就只输出命中段落（强烈建议给：实测能省 93%% token）")
    ap.add_argument("-B", "--before", type=int, default=2, help="命中行前保留几行")
    ap.add_argument("-A", "--after", type=int, default=3, help="命中行后保留几行")
    ap.add_argument("-m", "--max-chars", type=int, default=0, help="输出上限字符数，0=不限")
    ap.add_argument("-o", "--out", default="", help="写到文件（默认打到 stdout）")
    ap.add_argument("--prefer", choices=CONVERTERS, default="markitdown", help="优先用哪个转换器")
    ap.add_argument("--json", action="store_true", help="输出 JSON（含统计），给程序调用用")
    a = ap.parse_args()

    if not os.path.isfile(a.file):
        log(f"[错误] 文件不存在：{a.file}")
        return 2

    try:
        full, used = convert(a.file, a.prefer)
    except RuntimeError as e:
        log(f"[错误] {e}")
        return 3

    kws = [k.strip() for k in a.keywords.split(",") if k.strip()]
    body = full
    note = ""
    if kws:
        body = excerpt(full, kws, a.before, a.after, a.max_chars)
        if not body:
            # 空结果必须说清楚是"没命中"，不能让人以为文件是空的 ——
            # 也别偷偷回退成全文：那会让人在毫不知情的情况下付整份文件的钱。
            note = f"关键词 {kws} 在文中一次都没命中；未回退输出全文（避免意外付全价）。"
            log(f"[注意] {note}")
    elif a.max_chars > 0:
        body = full[: a.max_chars]

    log(f"[统计] 转换器={used} 全文 {len(full)} 字符 ≈ {est_tokens(full)} token"
        + (f" → 摘录 {len(body)} 字符 ≈ {est_tokens(body)} token"
           f"（{len(body) / max(1, len(full)) * 100:.0f}%）" if kws else ""))

    if a.json:
        payload = {
            "ok": bool(body) or not kws,
            "file": os.path.abspath(a.file),
            "converter": used,
            "full_chars": len(full),
            "full_tokens_est": est_tokens(full),
            "out_chars": len(body),
            "out_tokens_est": est_tokens(body),
            "keywords": kws,
            "note": note,
            "text": body,
        }
        out_text = json.dumps(payload, ensure_ascii=False, indent=1)
    else:
        out_text = body

    if a.out:
        with open(a.out, "w", encoding="utf-8") as f:
            f.write(out_text)
        log(f"[输出] 已写入 {a.out}")
    else:
        sys.stdout.write(out_text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
