---
name: uking-docx
description: 从零写一份新的 .docx（Word/WPS 可开）：周报/报告/合同/简历/方案等。纯 std 零依赖。⚠️ 要改用户**已有**的 Word 别用这个（会丢页眉页脚/字体/编号），改用 uking-office-edit。
---

# U-King 出真 Word（uking-docx）

## 何时用
用户要做 Word 文档、报告、周报、合同、简历、说明书等，需要能直接打开/继续编辑的 .docx 成品。

## 核心用法（Markdown → 真 .docx）
1. 先跟用户对齐大纲/要点，再动手。
2. 用 write_file 把正文写成一个 Markdown 文件（比如 `doc.md`）——Markdown 最自然：
   - `#` / `##` / `###` 标题、`- ` 列表、普通段落、`**加粗**`
   - 表格：`| 列A | 列B |` +下一行 `| --- | --- |`
   - 配图：`![说明](图片绝对路径)`（先用 generate_image 出图，路径用 Windows 风格 `C:/…/x.png`）
3. 用 run_command 生成：
   ```
   node ~/.uking/skills/uking-docx/scripts/gen-docx.mjs --md doc.md --out 报告.docx --json
   ```
   输出 `{"ok":true,"file":"…报告.docx"}`。告诉用户路径，可直接 Word/WPS 打开编辑。

## 结构化写法（可选，替代 Markdown）
也可写 `doc.json` 用 `--in doc.json`：
```json
{ "title":"标题", "blocks":[
  {"type":"heading","level":1,"text":"一级标题"},
  {"type":"paragraph","text":"正文，支持 **加粗**"},
  {"type":"bullets","items":["要点1","要点2"]},
  {"type":"table","rows":[["表头A","表头B"],["1","2"]]},
  {"type":"image","path":"C:/…/chart.png"} ] }
```

## 兜底
用户只想快速看看、不要 Word，就直接产出 Markdown 文本给他看。

## 什么时候**不该**用这个包

用户给了一份**已有的** .docx 让你改其中的文字 —— 那要用 **uking-office-edit**：

```bash
node <uking-office-edit>/scripts/edit-office.mjs 合同.docx --replace "旧文本=>新文本" --json
```

用本包重新生成，会把他的公司模板（页眉页脚、字体、编号、样式）全丢掉，
而且客户是打开文件那一刻才发现的。只有「结构性改动」（加删段落、改表格结构）
才值得重新生成，且要先跟用户说清楚格式会重来。
