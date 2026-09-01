---
name: uking-vision
description: 看图 + 读 PDF。图片/截图/照片要 OCR/识别文字/提取文字/看图说话/描述图片内容/读发票/合同/表格/题目/报错截图，或 PDF/文档/报告要解析/总结/提取内容时使用。给 DeepSeek 这类看不了图的模型当眼睛。
---

# U-King 看图（OCR + 图像理解 + 定位 + 读文档）

很多模型（尤其 **DeepSeek** 全系）**只会处理文字，收不了图片**。本技能给它当「眼睛」：把图片交给一个会看图的模型读成文字，再交回你（DeepSeek）继续干活。**你的对话模型不用换、省钱路由不动**，只在遇到图片时调一下本技能。

## 何时用本技能

- 用户发来 **图片 / 截图 / 照片**，让你识别、看、读；
- **OCR / 文字识别 / 提取图里的文字**；
- 读 **发票 / 合同 / 表格 / 题目 / 快递单 / 报错截图 / 网页截图**；
- **看图说话 / 描述图片内容**；
- 问「**这张图里写的是什么 / 这是什么 / 图里有几个 XX**」。

## 🔴 一条最重要的用法：带着目的问

**别泛泛地丢一张图进去。** 实测（`scripts/bench.mjs`，同一图同一模型跑 3 遍）：

- 泛问「描述这张图」→ 全体平均命中 **81%**；带上你要找什么 → **88%**。
- 弱模型上差距是断崖式的：同一张 2400px 宽的截图，泛问 **0/7**（而且会**整页编造**），带意图 **5.7/7**。

```bash
# ✅ 好：你知道自己要什么
node scripts/see-image.mjs 报错.png --ask "这个报错的完整错误码和堆栈第一行是什么？"

# ⚠️ 退而求其次：真的只是"看看这是啥"
node scripts/see-image.mjs 图.png
```

**「丢内容」的一大半根因不是模型不行，是没告诉它你要找什么。**

## 用法（有 Node 直接跑）

```bash
# 带目的问（首选）
node scripts/see-image.mjs ./报错.png --ask "这个报错是什么原因，怎么修？"

# OCR 模式：逐字转录图里所有文字（发票/表格/文档首选）
node scripts/see-image.mjs ./发票.jpg --ocr

# 默认：描述图片 + 顺带把图里的文字读出来
node scripts/see-image.mjs ./截图.png

# 机器可读：--json 出 {ok,text,model,mode,elapsed,size}
node scripts/see-image.mjs ./图.png --ocr --json

# 也能直接给公网图片链接
node scripts/see-image.mjs "https://example.com/pic.jpg"
```

**典型「搭配 DeepSeek 干活」流程**：用户贴图 → 你先 `--ask` 或 `--ocr` 拿到图里的内容 → 再用这段文字（你 DeepSeek 本体）分析、总结、写代码、算账。视觉模型只被调一次（几厘钱），主力活还是 DeepSeek 干。

## 定位 + 二次取证（大图小字专用）

图太大、字太小、第一遍没读准时，**别整张重看** —— 先定位，再裁那一块放大重看：

```bash
# ① 定位：出原图像素坐标（可直接喂给 --region）
node scripts/see-image.mjs 长截图.png --locate "底部的ICP证号那一行；绿色提示条"
#   底部的ICP证号那一行   [1128,847,1272,861]   ICP证：示字B2-20240917
#   绿色提示条            [946,694,1454,741]    …

# ② 二次取证：只看那一块
node scripts/see-image.mjs 长截图.png --region 1128,847,1272,861 --ocr
#   ICP证：示字B2-20240917
```

两个要知道的实现细节：

- **模型返回的框不是像素，是 0~1000 归一化坐标**（Qwen-VL 系列的约定）。脚本按图片真实宽高换算成像素，并把判定结果放进 `--json` 的 `space` 字段（`0-1000` / `0-1` / `pixel`），**坐标对不对你能自己核**，不用信我们。
- `--region` 用 **ffmpeg** 裁（全格式通吃，U-King「厨具工具箱」里可一键装）。没装会明确报错，不静默降级。

**裁剪不是万灵药**，两条实测边界：

- **能救的**：被降采样坑死的模型 —— `qwen3.5-ocr` 整幅漏读地址栏，裁完就中。
- **救不了的**：形近字偶发误读。149×14 的小图上见过一次把「示字」读成「京字」，但复跑 5 次全对；**放大 4 倍同样 5 次全对，没有改善** —— 所以这是 ~1/10 的随机抖动，不是分辨率问题，**别指望靠放大解决**（正因如此本脚本不做自动放大：加一个没被证据支持的处理只是自欺）。关键字段要保准，就复跑一次比对。

默认模型在 2400×2908 的长截图上本来就不需要裁。

## 读 PDF（`read-pdf.py`）

用户发来 **PDF**、让你读 / 解析 / 总结 / 提取 PDF（合同、报告、说明书、论文、扫描件）时用这个。**正解是先把 PDF 转成文字再交给你，不是让模型直接看页图**（页图对表格/公式/多栏易乱、长文档还巨贵）：

```bash
# 读整份 PDF -> Markdown（数字 PDF 直接抽文字=快准免费；**表格默认按行列还原**）
python scripts/read-pdf.py ./合同.pdf

# 带上下游要回答的问题（focus hint）：扫描页 OCR 会重点保真相关字段
python scripts/read-pdf.py ./报表.pdf --ask "丙项目的预算和已用分别是多少"

# 大文件只读前 N 页（省时省钱，先看开头再决定）
python scripts/read-pdf.py ./厚报告.pdf --max-pages 10

# 机器可读：--json 出 {ok,pages,text_pages,ocr_pages,tables,markdown}
python scripts/read-pdf.py ./doc.pdf --json

# 明确不做 OCR（只要数字文字、扫描页跳过，最省）
python scripts/read-pdf.py ./扫描件.pdf --no-ocr

# 退回旧行为（纯文本流，不还原表格）——排障用，日常别加
python scripts/read-pdf.py ./doc.pdf --no-tables
```

**流程**：用户贴 PDF → 你 `python scripts/read-pdf.py <pdf>` 拿到 Markdown → 再用这段文字（你 DeepSeek 本体）总结/问答/翻译/抽取。数字 PDF 抽文字**免费**，只有扫描页才按页调视觉模型（几厘钱/页）。首次会自动装 PyMuPDF（单包 ~15MB，一次性）。

### 🔴 表格：数字全在，关系全丢

`page.get_text("text")` 会把表格压成一维文本流。**字一个不少，但「这个数是哪一行哪一列的」没了** —— 这是最阴的一类损失，因为任何「关键词在不在」的检查都会显示满分。

实测（`node scripts/doc-bench.mjs`，题目全部必须知道行列归属才能答对，下游用 deepseek-v4-flash 真答题）：

| 抽取方式 | 密表（每行数值连续） | **稀疏表（有空单元格）** |
|---|---|---|
| 旧：纯文本流 | 14/14 (100%) | **9/18 (50%)** |
| **新：表格还原（默认）** | 14/14 (100%) | **18/18 (100%)** |
| markitdown（uking-office-read 用的） | — | 15/18 (83%) |

两条结论：

1. **差距只在稀疏表上出现。** 密表里每行数值连续，强模型能自己把表重建回来；一旦有空单元格，文本流里**不留任何占位**，于是「少了哪一列」无从判断 —— 下游会拿邻格的数当答案。实测原话：问「丙项目的预算是多少」，答「丙项目的【预算】是 4,100。」——4,100 是隔壁「已用」列，预算其实是空的。**答得斩钉截铁。**
2. markitdown 会多造一列、并把多行单元格的第二行甩出表外（「超支风险解除」变成表后的独立段落），所以它那 3 道错题全在多行单元格上。

## 参数（see-image.mjs）

| 参数 | 说明 |
|---|---|
| `<图片>` | 第一个位置参数：本地路径 或 http(s) 链接 或 data URL。也可 `--image <路径>` |
| `--ask "问题"` | **首选**。只回答指定问题（不传就默认「描述+读字」） |
| `--ocr` | OCR 模式：一字不差逐字转录，保留换行（读文档/发票/表格用） |
| `--locate "目标"` | 定位模式：出 `{what,bbox,text}`，`bbox` 已换算成原图像素 |
| `--region x1,y1,x2,y2` | 先按像素框裁剪再看（二次取证）。需 ffmpeg，仅本地文件 |
| `--json` | 输出契约 JSON `{ok,text,model,mode,elapsed,size,space?,items?,fallback_from?}`；否则直接打印文字 |
| `--model <id>` | 换模型。默认走三棒链 `qwen3.7-flash` → `qwen3.7-plus` → `kimi-k3`（走了替补会在 `fallback_from` 里说明；显式指定 `--model` 时**不**自动换）。整条链有 240s 总预算，不会假死。**纯文本模型会被当场拒绝**，见下 |
| `--key sk-...` | 手动指定 Key（一般不用，自动读 device.json） |

## 替补链（不用管，出问题时才看得见）

默认走三棒，**两棒各挡一类故障，不是「第二好」「第三好」**：

| 棒次 | 模型 | 挡什么 | 实测 |
|---|---|---|---|
| 主力 | `qwen3.7-flash` | — | 合计 100%，长截图 4/4，中位 ~11s |
| ① | `qwen3.7-plus` | 单个模型抽风 | 95%，快（4~6s）。**同属阿里那条路由**，整条腿断时一起断 |
| ② | `kimi-k3` | **整条路由断** | 裸名=月之暗面直连，账号/端点跟阿里无关。**慢**：长截图中位 61s、峰值 129s，泛问命中 1~4/4 不稳；`--ask` 稳定 4/4 |

- 走了替补，`--json` 里会有 `fallback_from`；stderr 上也会明说换了谁。**降级不许隐身。**
- 整条链有 **240s 总预算**，剩不下 15s 就不再起新的一棒 —— 否则三棒串起来最坏能假死 9 分钟。
- 显式 `--model` 时**不**自动换（你选错了要能看见）。

## 🔴 别把图发给纯文本模型（`--model` 用错会得到编的答案）

纯文本模型收下 `image_url` **不报错**：HTTP 200、`choices` 齐全，正文是一个编出来的答案。
2026-08-16 用 `bench/fixtures/license.png`（法定代表人正解「张示例」）实测：

| 模型 | 问「法定代表人姓名」 | 泛问「描述这张图」 |
|---|---|---|
| `qwen-turbo` | **「张三」**（凭空编） | — |
| `qwen-plus` | **「张三」**；换个问法就当常识题答 600 字，全程不提自己看不见 | 老实说「我无法查看图片」 |
| `qwen3.7-flash` / `qwen3.7-plus` | 「张示例」✅ | ✅ |

所以脚本有**三道**防线，按可靠性排序：

1. **发出去之前**按内置清单拦（主防线，见下「名字最容易混的三对」）；
2. **发出去之前**查 dsh 的 pi-ai catalog（运行时，补第 1 条想不到的那些 —— 实测多拦 192 个）；
3. 正文里的拒答句式当最后一道网（上表说明它**只在泛问时管用**，别指望它）。

🔴 第 2 条**只用来加拦，绝不用来放行**：默认模型 `qwen3.7-flash` 压根不在 catalog 里，
把「不在里面」当成纯文本的证据会把主力路径当场拦死。同一裸名有矛盾条目时也一律放行
（559 个裸名里有 5 个自相矛盾）。跑道 `scripts/check-vision-gate.mjs` 把这两条钉死了。

名字最容易混的三对：

- `qwen-plus` / `qwen-turbo` ❌ ← 老一代纯文本；`qwen3.6-plus` / `qwen3.7-plus` ✅ 才收图
- `qwen3.7-max` ❌ ← 纯文本；同族 `qwen3.7-plus` ✅
- `glm-5` / `glm-5.1` / `glm-5.2` ❌；`glm-5v-turbo` ✅（多一个 v）

**模型能力从哪来**（别从这份文档猜，它会过期）：本机装了 dsh 时，
`@earendil-works/pi-ai` 的 catalog 里每个模型条目带 `input:["text"]` / `["text","image"]`，
是目前唯一一份机器可读的模态真相源。**脚本运行时会自己查它**（上面第 2 道防线），
下面这条命令只是给人排障用的 —— 想知道某个模型闸门认不认，直接跑它：

```bash
node -e 'const fs=require("fs"),p=require("path");
const d=p.join(process.env.HOME||process.env.USERPROFILE,".uking/runtime/node/node_modules/@deepseek-ai/dsh/node_modules/@earendil-works/pi-ai/dist/providers/data");
for(const f of fs.readdirSync(d)){const j=JSON.parse(fs.readFileSync(p.join(d,f),"utf8"));
for(const a in j)for(const id in j[a]){const m=j[a][id];
if(Array.isArray(m.input)&&m.input.includes("image"))console.log(f.replace(".json",""),id)}}'
```

⚠️ 它**覆盖不到** `qwen-plus` / `qwen-turbo` / `qwen3.7-flash`（catalog 里没这几个 id），
所以脚本里的名单是「catalog + 我们自己实测」两个来源合的，不能只靠一边。

## 支持的图片

- 格式：**JPG / PNG / WEBP / GIF**；
- 大小建议 ≤ 8MB、长宽 ≤ 4096 像素（过大会变慢或被上游拒）。

## API Key（通常无需手动给）

按优先级取：① `--key sk-...` ② 环境变量 `XIAPAN_API_KEY` ③ `~/.uking/device.json` 的 `key`（装了 U-King 就有，设备专属、恒定）。**本脚本内不含任何 Key**，可随意分发。

## 为什么默认用 qwen3.7-flash（有跑道，可自己复跑）

`node scripts/bench.mjs --cases bench/cases.json --repeat 3` —— 三类**全合成夹具**（零隐私，`bench/gen-fixtures.mjs` 生成），needles 是图里客观存在、必须被读出来的字符串，不做模糊匹配：

| 模型 | 证照字段 | 大图小字(2400px) | 长截图(2400×2908) | 耗时中位 |
|---|---|---|---|---|
| **qwen3.7-flash** | 8/8 | 7/7 | **4/4** | 13.1s |
| qwen3-vl-flash | 8/8 | 6~7/7 | 1.8/4 | 10.6s |
| qwen-vl-max | 8/8 | 5~6/7 | 0.5/4 | 9.1s |
| qwen-vl-plus | 8/8 | 5/7 | 未测 | 6.8s |
| qwen3.5-ocr | 泛问 8/8 · 带意图 5/8 | 3~5/7 | 1/4 | 5.4s |
| MiniMax-M3（旧默认） | 6.7/8 | **泛问 0/7** | 未测 | 6.6s |

三条结论：

1. **长截图是分水岭**：只有 qwen3.7-flash 扛住（4/4），其余从 44% 一路崩到 13%。
2. **旧默认 MiniMax-M3 的失败模式是「编」不是「漏」** —— 泛问那张宽截图三遍全 0/7，还编出不存在的按钮、账号和公司名。confidently wrong 比读不出来危险得多，这是换默认的直接原因。
3. **专用 OCR 模型别用于长图**：`qwen3.5-ocr` 在 96 行密集长图上撞 token 上限被截断、页脚整段丢失。

**整页逐字转录**是另一个口径，结论不同 —— 所以 `read-pdf.py` 的扫描页 OCR 单独选了 `qwen3-vl-flash`：

| 模型 | 转全 | 页脚 | 耗时 | 输出 token |
|---|---|---|---|---|
| **qwen3-vl-flash** | 96/96 | ✓ | **42s** | 3094 |
| qwen-vl-max | 96/96 | ✓ | 71s | 2905 |
| qwen3.7-flash | 96/96 | ✓ | 44s | 7052（同样内容多花 2.4×） |
| qwen3.5-ocr | 95/96 | ✖ | 38s | 撞上限截断 |

模型每隔几周换一批，**别信这张表的绝对数字，跑道自己跑一遍**：

```bash
node bench/gen-fixtures.mjs                                   # 图片夹具（需 playwright）
node bench/gen-doc-fixtures.mjs                               # 文档夹具（PDF）
node scripts/bench.mjs --cases bench/cases.json --repeat 3    # 图片跑分
node scripts/doc-bench.mjs --pdf bench/fixtures/report-sparse.pdf --repeat 3  # 文档跑分
```

**两条跑道的判分口径不一样，别混用**：图片跑道判「这串字有没有出现」（needle）；文档跑道判「下游模型答不答得对」（结构相关 QA）。表格上前者永远满分，测不出任何东西。

> `bench/` 与 `scripts/*bench.mjs` 是**开发期跑道，不随技能分发**（`skillpack.rs` 的 `Pack.files` 只列了 SKILL.md + 两个脚本）。夹具是生成的，不入库。

**已知不可用**：百度 `ernie-4.5-turbo-vl` / `ernie-5.0` 在虾盘云上无渠道；`ernie-4.5-turbo-128k` 是纯文本、直接拒图。
