---
name: uking-browse
description: 打开网页/读网页。用户说「打开这个网页」「看看这个网站写了什么」「把这篇文章总结一下」「查一下这个链接」「打开我刚才做的那个文件」时使用；抓取转 Markdown 供总结。
---

# U-King 开网页 / 读网页（uking-browse）

## 两件事，别搞混
| 用户想要 | 用哪个 |
|---|---|
| 「**打开**这个网页 / 打开我刚做的那个文件」——他要**眼睛看到** | `open-url.mjs` |
| 「这个链接里写了什么 / 帮我总结这篇」——**你**要读进来 | `fetch-page.mjs` |

## 打开（给人看）
```bash
node ~/.uking/skills/uking-browse/scripts/open-url.mjs https://u-king.org --json
node ~/.uking/skills/uking-browse/scripts/open-url.mjs "D:/活/图纸.预览.svg" --json
```
网址用默认浏览器开，本地文件用默认程序开。**做完一件办公活，顺手把产物开给用户看** ——
文件路径打在对话里，客户往往找不到，更不会自己去开。

⚠️ 别自己敲 `start`：Windows 的 `start` 是 cmd 内建、第一个参数会被当窗口标题吃掉，
直接调常常只弹出一个空窗口。用这个脚本。

## 读进来（给你看）
```bash
node ~/.uking/skills/uking-browse/scripts/fetch-page.mjs https://example.com/article --json
node ~/.uking/skills/uking-browse/scripts/fetch-page.mjs <url> --max-chars 20000    # 正文长时放宽
node ~/.uking/skills/uking-browse/scripts/fetch-page.mjs <url> --links              # 顺带列出页面链接
```
输出 `{"ok":true,"title":"…","text":"…markdown 正文…","chars":N,"truncated":bool}`。

脚本在本地就把 script/style/nav/footer 剥掉再转 Markdown，一个 200KB 的页面通常只剩 3~8%。
**不要用 `curl <url>` 把整页 HTML 读进上下文** —— 又贵又常常直接撑爆。

## 要点
- `truncated: true` 说明正文被截断了，需要全文就加 `--max-chars`，别拿半截内容下结论。
- 抓不到（403 / 超时 / 要登录 / 纯 JS 渲染的站）→ **如实告诉用户抓不到**，
  绝不根据网址名字和常识编造页面内容。这是本包最容易出的事故。
- 要搜索（「帮我搜一下…」）本包做不到：它只会按你给的网址取页面，不会找网址。
  没有网址就问用户要，或让用户自己搜完把链接贴过来。

## 配合
抓到正文 → 要出报告走 `uking-docx`、要出表格走 `uking-xlsx`、要发邮件走 `uking-mail`。
产物出完 → 再用 `open-url.mjs` 开给用户看，形成闭环。
