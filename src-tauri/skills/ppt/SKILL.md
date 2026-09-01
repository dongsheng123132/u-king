---
name: uking-ppt
description: 做 PPT/幻灯片/演示。用 gen-pptx.mjs 出有设计感的真 .pptx（内置主题配色+封面/章节/内容/图文/金句版式），也可出可预览 HTML 幻灯片。给 PPT/文档专家用。
---

# U-King 做有设计感的 PPT（uking-ppt）

## 何时用
用户要做 PPT/演示/路演/汇报/课件，需要能直接打开、**看起来专业**的成品。

## 首选：出真 .pptx（gen-pptx.mjs，内置设计系统，零依赖）
生成器自带主题配色 + 5 种版式，别再只堆纯文字页——**用好版式**出片才不寒酸。

**步骤**
1. 先跟用户对齐大纲（几个部分、每部分讲什么），确认后再动手。
2. 用 write_file 写 `deck.json`：**首页 cover、每个部分前插 section 分隔页、要点用 content、关键结论用 quote、末页 end**。挑一个贴合主题的 `accent` 主题色。
   ```json
   {
     "title": "整体标题",
     "accent": "indigo",
     "slides": [
       { "type":"cover",   "title":"主标题", "subtitle":"一句话副标题", "footer":"2026 · 团队/作者" },
       { "type":"section", "title":"第一部分 · 背景", "number":"01" },
       { "type":"content", "title":"页标题（一句话观点）", "bullets":["要点1","要点2","要点3"] },
       { "type":"content", "title":"图文页", "bullets":["左侧要点"], "image":"C:/…/hero.png" },
       { "type":"quote",   "text":"一句有力的结论/金句", "by":"—— 出处（可选）" },
       { "type":"end",     "title":"谢谢观看", "subtitle":"Q & A / 联系方式" }
     ]
   }
   ```
3. 生成：`node ~/.uking/skills/uking-ppt/scripts/gen-pptx.mjs --in deck.json --out 演示.pptx --json`
   → 出**一对**产物，两个都要给用户：
   - `演示.pptx` —— 交付物，PowerPoint/WPS 可开可编辑
   - `演示.预览.html` —— **同一份大纲、同一套版式渲染的网页版**，软件里点「预览」秒开就能看到长什么样
     （自包含：零外链、配图 base64 内联，断网也开得了，也能单独发给别人；Ctrl+P 可直接印成 PDF）

   **先让用户看预览再问要不要改**，别让他为了看一眼成果去启动 Office。
   `--json` 返回 `{ok, file, html, slides, accent}`；确实不想要那份网页版就加 `--no-html`。

**版式（type）**：`cover` 封面（主题色满版）· `section` 章节分隔（大编号）· `content` 内容页（标题+短色条+要点，可带 `image` 右侧配图）· `quote` 金句页 · `end` 结尾页。省略 type 时：第 0 页=cover、有 bullets=content、只有 subtitle=section。
**主题色 accent**：hex（如 `2563EB`）或命名 `indigo/teal/rose/amber/emerald/slate/blue/violet`。

## 做好 PPT 的要点（重要）
- **一页一个观点**，标题直接写观点（别写「概述」这种废话标题）。
- 每页要点 **3–5 条、每条一行**，别写大段话。
- 多用 `section` 把内容分段，逻辑清楚；关键结论用 `quote` 页强调。
- 配图用 generate_image 出图后填 `image`（绝对路径，Windows 用 `C:/…`）。

## 关于「边做边预览」
**不用再手写 slides.html 了** —— 上面第 3 步已经自动出了一份 `演示.预览.html`，
和 .pptx 同源同版式。改大纲后重跑一次 gen-pptx.mjs，两份一起更新，不会漂移。

（历史做法是让 AI 用 write_file 现写一个 Tailwind CDN 的 slides.html：既多烧一轮 token，
又和真正的 .pptx 长得不一样 —— 客户照着预览点头，打开 pptx 发现是另一个东西。）

## 文档（非 PPT）
用户要 Word/报告/周报走 uking-docx（真 .docx）；纯文本要点用 Markdown。

## 什么时候**不该**用这个包

用户给了一份**已有的**文件让你改其中的文字 —— 那要用 **uking-office-edit**：

```bash
node <uking-office-edit>/scripts/edit-office.mjs 文件 --replace "旧文本=>新文本" --json
```

用本包重新生成，会把他原来的模板/母版/样式全丢掉，而且客户是打开文件那一刻才发现的。
只有「结构性改动」（加删幻灯片、改版式）才值得重新生成，且要先跟用户说清楚格式会重来。
