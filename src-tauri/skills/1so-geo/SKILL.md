---
name: 1so-geo
description: 一搜商答 / 1so —— 本地 GEO 工具。当用户要「看自己公司在互联网/AI 里的样子」「做 GEO/被 AI 收录/被 AI 引用」「一键搜全网看自己」「各大模型认不认识我」「生成企业主页/官网」「优化在 AI 搜索里的可见度」「行业高频问答」时使用。命令：`node bin/1so.mjs scan`（40+渠道体检面板，客户自查+自动判读）、`detect`（单后端 AI 眼里的你）、`aicheck`（对接各大模型跑一遍问答，看各家AI认不认识你，OpenRouter/BYOK扣客户自己费用）、`ingest`（本地资料→知识卡）、`generate`（→企业主页 HTML+JSON-LD+地图）、`optimize`（补内容清单）、`questions`（行业高频问答→多表达）、`run`（一条龙）。LLM 后端可插拔：`--provider uking`（本机claude/codex）/`openrouter`（一key通全网模型）/`openai`（虾盘云）/`bl`（百炼）。产物落 <项目>/site/（可直接发客户）与 .1so/。
---

# 一搜商答 / 1so（U-King GEO 技能）

把老板本地资料变成 AI 能读懂、可被引用的商家答案；并检测公司在互联网 / AI 里的样子。理念与总规划见工具目录里的 `GEO百科-产品规划.md`。

## 何时用本技能

- 「查查我公司在网上/AI 里是什么样」→ `scan`（40+ 渠道体检面板）+ `detect`（AI 可见度报告）
- 「帮我做个企业主页 / 让 AI 能搜到我」→ `ingest` → `generate`
- 「我这行业客户都问什么，我该怎么答」→ `questions`
- 「怎么优化才能被 AI 推荐」→ `optimize`

## 独立性（重要）

本 skill **完全自包含、可随时删除**：只写自己的 `<项目>/` 目录 + 系统临时文件；对 `~/.uking` 只**读取** `device.json` 取 key，**零写入**；不引用、不修改 U-King 任何核心/配置/其它 skill。删掉本文件夹即干净移除，不影响 U-King 现有功能。

## LLM 后端（可插拔，按需选）

1. `--provider uking` —— 调本机 U-King 已装好的 `claude`/`codex`（虾盘云 key 配好鉴权，客户机零服务器依赖）。`--agent codex` 可切。
2. `--provider openrouter` —— OpenRouter，**一个 key 通全网模型**（GPT/Claude/Gemini/DeepSeek…）。key 读 `OPENROUTER_API_KEY` 或 `--key`；`--model openai/gpt-4o-mini` 之类指定。
3. `--provider openai` —— 虾盘云等 OpenAI 兼容端点，key 自动读 `~/.uking/device.json`。**需网关放开该 device key 的文本通道**（目前默认只开作图/视频）。
4. `--provider bl` —— 本机百炼 CLI（开发机）。

先自检：`node bin/1so.mjs doctor --provider <后端>`

## 典型用法

```bash
# 客户目录：把资料丢进 <客户>/materials/（.txt/.md 等）
node bin/1so.mjs run --project ./客户名 --provider uking          # 一条龙：提炼→扫全网→检AI→生成主页→优化→预览
# 或分步：
node bin/1so.mjs scan     --project ./客户名 --name "公司名" --region "深圳"   # 体检面板（可直接发客户自查）
# 对接各大模型跑一遍问答（用客户自己的 OpenRouter key，扣客户费用）：
OPENROUTER_API_KEY=sk-or-... node bin/1so.mjs aicheck --project ./客户名 --name "公司名"
node bin/1so.mjs ingest   --project ./客户名 --provider uking
node bin/1so.mjs questions --project ./客户名 --provider uking --merge         # 行业高频问答，并入内容
node bin/1so.mjs generate --project ./客户名                                  # 企业主页（含三大地图）
node bin/1so.mjs preview  --project ./客户名
```

## 产物（发客户 / 上线）

- `site/体检面板.html` —— 40+ 渠道自查仪表盘，**可直接发给客户**（独立 HTML，无依赖）。
- `site/index.html` —— 企业主页（语义 HTML + JSON-LD 结构化数据 + llms.txt + 高德/百度/腾讯地图）。
- `.1so/报告-AI眼里的你.md`、`.1so/优化建议.md`、`.1so/行业问答.md` —— 给客户看的报告。
