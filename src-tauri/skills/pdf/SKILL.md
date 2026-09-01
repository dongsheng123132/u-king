---
name: uking-pdf
description: 导出 PDF。「导出 PDF」「转成 PDF」「做成 PDF 发给客户」「PDF 版本」「不要让对方能改」时使用。Markdown/HTML 走系统 Edge/Chrome，Word/Excel/PPT 走 LibreOffice 保版式。
---

# U-King 导出 PDF（uking-pdf）

## 何时用
客户要的是**最终交付形态**：合同要发出去、报价单要给甲方、报告要存档、简历要投出去。
PDF 的意义是「对方打开跟我看到的一模一样、而且改不了」。

## ★ 两条引擎，按输入分流（先看清楚再动手）

| 输入 | 引擎 | 要不要装东西 |
|---|---|---|
| `.md` `.html` | **Edge / Chrome headless** | **不用**。Win10/11 出厂自带 Edge，实测 0.39s，中文完整且**文字可搜索**（不是图片） |
| `.docx .xlsx .pptx .doc .odt …` | LibreOffice | 要装（约 400MB） |

**AI 写完的报告九成是 Markdown —— 那条路零安装，优先往那边走。**
要一份 PDF 报告时，正确做法通常是：**直接写 .md → 转 PDF**，而不是先出 .docx 再转
（后者平白多一个 LibreOffice 依赖）。除非用户明确要一份**能改的 Word**。

## 核心用法
```bash
node ~/.uking/skills/uking-pdf/scripts/to-pdf.mjs --check --json      # 先看这台机器有哪条引擎
node ~/.uking/skills/uking-pdf/scripts/to-pdf.mjs 报告.md --json       # 零安装路径
node ~/.uking/skills/uking-pdf/scripts/to-pdf.mjs 合同.docx --json     # 需要 LibreOffice
node ~/.uking/skills/uking-pdf/scripts/to-pdf.mjs 报表.xlsx --out D:/发货/报表.pdf --json
node ~/.uking/skills/uking-pdf/scripts/to-pdf.mjs 页面.html --engine chromium --json
```
输出 `{"ok":true,"file":"…pdf","size":N,"engine":"Edge (headless)","ms":484}`。

Markdown 支持标题 / 加粗斜体 / 行内码 / 有序无序列表 / **表格** / 引用 / 分隔线 / 链接，
内置 A4 中文排版样式（字体点名了微软雅黑·苹方·宋体 —— Chromium headless 的默认字体
在部分客户机上会把中文渲染成方框）。

## 要点
- **`--check` 先探一下**，别等转到一半才发现没引擎。
- 要**同时给可编辑版和 PDF**：`uking-docx` 出 .docx + 本包出 .pdf，两个都给用户 ——
  他多半还要再改一版，只给 PDF 等于把他锁死。
- 转完把**大小**报给用户；PDF 只有几百字节基本是空的（脚本已经会拦）。

## 🔴 LibreOffice 装不上是常态，不是意外
2026-08-04 实测：**非管理员会话里 `winget install ... --silent` 直接 1603 失败，UAC 提示压根不弹**
（winget 在非提权控制台不会自动提权），客户只会看到「没装上」。试过 7z 解 MSI 绕开管理员 ——
出来 19494 个扁平文件、缺 `program/` `share/`，soffice 跑得起来但转不出任何东西。**这条路不通。**

所以：**能走 Markdown 就别走 Office 文件**。要一份 PDF 报告，直接写 `.md` → Edge 转，
零安装、不需要管理员、0.5 秒。只有「客户拿来一份带公司模板的 .docx 要转」才非 LibreOffice 不可，
那时如实告诉他要用**管理员身份**的终端装。

## 没装 LibreOffice、又必须转 Office 文件
脚本直接报 `ok:false` + `how_to_fix`，**不会**退而求其次「把文字抽出来重排一份 PDF」——
那种东西客户打开才发现版式全没了，比直接说做不到坏得多。
两条出路如实转告用户：
1. 装 LibreOffice（厨具工具箱里点一下，或 `winget install TheDocumentFoundation.LibreOffice`）；
2. **如果这份文件是 U-King 自己生成的**，旁边通常有一份同源 `.预览.html`，
   转那个即可 —— 版式一致且零安装。

## 已经踩过的坑（脚本里都处理了，别绕开脚本自己敲命令）
1. **soffice 转换失败时照样退出码 0**。唯一可信的判据是「PDF 文件真的出来了且非空」。
2. LibreOffice 界面开着时第二个实例会直接退出 → 脚本用 `-env:UserInstallation` 开独立 profile。
3. 浏览器开着时 headless 会去复用已有实例然后什么都不打印 → 脚本给独立 `--user-data-dir`。

## 什么时候**不该**用这个包
- 要**读** PDF 内容 → `uking-office-read`（扫描件走 `uking-vision` 的 read-pdf 做 OCR）
- 要**改** PDF → 本包做不到。如实说明：正确做法是回去改源文件再重新导出。
