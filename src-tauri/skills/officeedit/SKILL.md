---
name: uking-office-edit
description: 改已有 Word/PPT/Excel 里的文字而不丢格式。「合同里的甲方改成…」「年份换一下」「表格里 X 替换成 Y」，或给了 .docx/.pptx/.xlsx 要改文字时用。从零写新文件用 uking-docx/ppt/xlsx。
---

# 改文档不丢格式（uking-office-edit）

支持 **.docx（Word）· .pptx（PPT）· .xlsx（Excel）**。

## 为什么必须用它，而不是「读出来再重新生成」

客户拿来的文件多半带着公司模板：页眉页脚、字体、编号、母版、图表。
读出来（uking-office-read）→ 改内容 → 用 uking-docx / uking-ppt / uking-xlsx 重新生成，
拿回去的是一份**白板文件**——模板全没了，而且客户是打开文件那一刻才发现的。

这个脚本只动文字：**未修改的部件直接复制原始压缩字节**，样式 / 页眉页脚 / 图片 / 编号
是**字节级相同**，不是"看起来一样"。

## 一句话用法

```bash
node <本技能目录>/scripts/edit-office.mjs 合同.docx --replace "甲方：张三=>甲方：李四" --out 合同-改.docx --json
node <本技能目录>/scripts/edit-office.mjs 方案.pptx --replace "2025=>2026" --json
node <本技能目录>/scripts/edit-office.mjs 报价.xlsx --replace "初稿=>终稿" --json
```

**三种格式一个脚本**：它们内部的「段落 → 文本片段」结构是同构的，只是标签名不同 ——

| 格式 | 改哪些部件 | `--all-parts` 追加 |
|---|---|---|
| .docx | 正文 | 页眉页脚（文件编号 / 日期常在这儿） |
| .pptx | 全部幻灯片 | 演讲者备注 |
| .xlsx | 共享字符串表（= 所有单元格文字） | 工作表内联字符串 |

改动多时用 JSON（正文里有引号、换行时也用它）：

```bash
node edit-office.mjs 报告.docx --map 改动.json --json
# 改动.json: [{"find":"2025年","replace":"2026年"},{"find":"初稿","replace":"终稿"}]
```

其它开关：

| 开关 | 作用 |
|---|---|
| `--in-place` | 直接改原文件（**自动留 `.bak`**，已有 .bak 时不覆盖） |
| `--all-parts` | 连页眉页脚一起改（公司模板的文件编号 / 日期常在那儿） |
| `--json` | stdout 出一行 JSON：`{ok, file, replaced:[{find,count}], missed, parts_preserved}` |

stdout 只出结果，日志走 stderr —— 可以直接接管道。

## 怎么用才不会白改

**要替换的文字必须和文档里完全一致**（含空格、标点、全角半角）。
最稳的做法是**先读原文、照着复制**：

```bash
python <uking-office-read>/scripts/read-doc.py 合同.docx -k "甲方,乙方,金额"
# 从输出里原样复制要改的那句，再交给 edit-office.mjs
```

**一处都没命中时脚本会退出码 1 并且不生成文件** —— 不会让你以为改好了。
`missed` 字段会列出没找到的每一条。

## 做不到的（如实说，别硬试）

| 做不到 | 为什么 / 该走哪 |
|---|---|
| **跨段落**的文字匹配 | 匹配以段落为单位。要改的话拆成两条分别替换 |
| 段内混排格式的精细保留 | 一段里前半句加粗、后半句不加粗，命中后整段跟第一个 run 的格式走。**保住整篇模板**和**保住段内混排**之间的取舍 |
| 加段落 / 删段落 / 改表格结构 / 换图片 | 这个包只替换文字。结构性改动请用 uking-docx / uking-ppt / uking-xlsx 重新生成 |
| `.doc` / `.ppt` / `.xls`（老二进制格式） | 不是 ZIP，脚本会直接报错。让客户先「另存为」新格式 |
| 加密 / 有密码的文档 | 解不开，也不该去解 |
| **Excel 的公式和数字** | 它们不在共享字符串表里，改不了。要改数值请用 uking-xlsx 重新生成 |
| **Excel 里同一个词只想改一处** | Excel 把相同文字共享成一条 —— 改它 = 所有用到该文字的单元格一起变。这是 Excel 的机制，不是 bug |
| PPT 的图表 / SmartArt 内部文字 | 那些在独立的图表部件里，本包不动（正因为不动，图表才不会坏）|

## 跟其它包的分工

- 读客户已有的文档 → **uking-office-read**
- 在客户已有的 Word / PPT / Excel 上改文字 → **本包**
- 从零写一份新 Word / Excel / PPT → **uking-docx / uking-xlsx / uking-ppt**

零 npm 依赖（只用 Node 内置 fs/zlib），不含任何 Key，不联网，纯本地改文件。
