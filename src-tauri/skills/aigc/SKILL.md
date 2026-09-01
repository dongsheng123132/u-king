---
name: uking-aigc
description: 虾盘云作图与视频工具。画图/作图/文生图/图生图/改图/生成图片/配图/生成视频/文生视频/图生视频/做短视频/漫剧/宣传片/文字转语音/配音/旁白/拼接视频时使用；命令与提示词工作法在正文。
---

# U-King AIGC（AI 作图 + AI 视频）

虾盘云作图与视频的本地工具。两种用法：**① 有 Node 直接跑脚本**（推荐，已封装好异步轮询、下载、国内域名改写等坑）；**② 没 Node 照「纯 curl 用法」手调**。

## 何时用本技能

画图 / 作图 / 文生图 / 图生图 / 改图 / 生成图片 / 给文章配图 / 生成视频 / 文生视频 / 图生视频 / 做个短视频。

## API Key（通常无需手动给）

脚本按以下优先级取 Key：
1. 命令行 `--key sk-...`
2. 环境变量 `XIAPAN_API_KEY`
3. `~/.uking/device.json` 的 `key` 字段（装了 U-King 就有，设备专属、恒定）

没充值会报「余额不足 / Invalid token」，去 https://u-claw.org.cn/recharge 充值（¥1 ≈ 50 万 token）。

## 提示词工作法（先补全，再出图 —— 本技能最值钱的部分）

用户通常只给一句话（「画只猫」「做个封面」「配张图」）。**别把这句话原样丢给脚本**——你要先把它补成一条**专业提示词**（主体 + 细节/材质 + 风格 + 光影 + 构图/镜头 + 画质词），并按用途**自动选好 `--size`**，再去调 `gen-image.mjs`。这正是本地技能包吊打在线版的地方：在线 web 版每次都要用户自己敲一长串上下文，你不用——你替用户想好。用户没特别要求时**别反问，直接按下表补全出图**；他要改再改。

### 万能提示词公式

`主体 + 细节/材质 + 风格 + 光影 + 构图/镜头 + 画质词`（如 `ultra detailed, 8k, cinematic lighting`）。中英文都行，gpt-image-2 中文也听得懂；追求写实/精细时用英文更稳，中国风/文字场景用中文更贴。

### 一句话 → 专业提示词（照抄这些套路）

| 用户说 | 你补全成的 `--prompt` | 建议 `--size` |
|---|---|---|
| 画只猫 | a fluffy orange tabby cat, studio portrait, soft cinematic lighting, shallow depth of field, ultra detailed, 8k | 1024x1024 |
| 做个美食封面（抖音） | 顶视角一碗热气腾腾的红烧牛肉面，油光质感，暖色调，背景虚化，高级感美食摄影，竖构图，超清 | 1536x2048 |
| 电商产品图 | 白底棚拍，一瓶护肤精华液，柔和反光，居中构图，干净背景，商业产品摄影，高清细节 | 1024x1024 |
| 小红书封面 | 生活方式场景，暖阳光线，ins 风，柔和色调，留白构图，清新高级，竖版 | 1024x1536 |
| 做个头像 | close-up portrait, clean background, soft rim light, high detail, professional avatar | 1024x1024 |
| 设计个 logo | minimal flat logo, simple geometric shapes, two-color, vector, clean white background | 1024x1024 |
| 做张海报 | bold poster design, strong composition, dramatic lighting, high contrast, marketing key visual（**文字后期再叠**） | 1024x1536 |
| 画个插画 | flat illustration, minimal, soft palette, clean vector style | 1024x1024 |
| 科技/发布会背景图 | futuristic tech background, glowing gradient, abstract geometry, dark theme, cinematic | 1536x1024 |

（这几行是模板，按用户的具体主体替换名词即可。用户点名了风格/颜色/相机就把它揉进公式对应位置。）

### 场景 → 尺寸速查（不用问用户，按用途自动选）

- 抖音 / 短视频 / 视频号封面：`1536x2048`（3:4 竖）
- 小红书 / 朋友圈长图 / 手机壁纸：`1024x1536`（2:3 竖）
- 头像 / 图标 / logo / 产品白底图：`1024x1024`（1:1）
- 横版 banner / 电脑壁纸 / 文章头图 / PPT 配图：`1536x1024`（3:2 横）
- 拿不准：`auto`

> ⚠️ `--size` 只填 `宽x高` 像素值，**别填 `3:4`/`16:9` 这种比例文字**（上游会报错拒绝出图）。要更精确的比例：先按最接近的档位出图，再裁剪/缩放。

### 风格短语库（按用户口味拼进 `--prompt`）

- 写实摄影：`photorealistic, cinematic lighting, 8k, ultra detailed`
- 扁平插画：`flat illustration, minimal, vector style, clean, soft palette`
- 3D 渲染：`3D render, octane render, soft studio light, C4D, subsurface`
- 国潮 / 中国风：`Chinese ink painting style, guochao, elegant, red and gold`
- 赛博朋克：`cyberpunk, neon lights, moody, rainy night, high contrast`
- 卡通 / 二次元：`anime style, cel shading, vibrant, clean lineart`
- 复古胶片：`vintage film photography, grain, warm tone, 35mm`

### ⚠️ 别硬做的（先给用户打预防针，别默默出废图）

- 图里要**准确的中文长句 / 二维码 / 公式 / 精确 LOGO 文字**：扩散模型做不出可扫的二维码、也常写错中文长句（2026-07 生产实锤）。带字的海报/封面：先出好背景图，再用其它工具（PS/Canva/网页）叠字，别指望模型把字写对。
- 要同一需求的多个方案：加 `--n 4` 一次出 4 张（文生图，1~4 张，各不相同）让用户挑；跨不同提示词/镜头的批量再用 `gen-batch.mjs`（见「批量」段）。

## 作图：scripts/gen-image.mjs

```bash
# 文生图
node scripts/gen-image.mjs --prompt "一只橘猫宇航员，电影感光影" --model gpt-image-2 --size 1024x1024 --out cat.png --json

# 一次出 4 张不同方案让用户挑 + 指定画质（--n 1~4，仅文生图；--quality low/medium/high/auto）
node scripts/gen-image.mjs --prompt "极简 logo，一只狐狸" --n 4 --quality high --out fox.png --json

# 图生图 / 改图（带参考图，--ref 可重复多张做融合）
node scripts/gen-image.mjs --prompt "把背景换成星空" --ref cat.png --out cat-space.png --json
```

参数：`--prompt`(必填) `--model`(默认且推荐 gpt-image-2；理论上可填任意模型 id，但客户端已只主推这一个，稳定性/成本都验证过) `--size` `--n`(一次出几张，1~4，**仅文生图**；同一提示词出多个方案时用) `--quality`(`low`/`medium`/`high`/`auto`；low 最省最快、high 最精细，gpt-image-2 已实测生效) `--ref`/`--image`(参考图，可重复) `--out`(默认 ./uking-image-<时间>.png) `--key` `--json`

> `--n` 多张时：第一张写 `--out`，其余在扩展名前插 `-2`/`-3`…（`cat.png` → `cat-2.png`）；`--json` 里 `files` 是全部路径、`file` 是第一张。图生图（带 `--ref`）固定 1 张。

> `--size` 必须是 `宽x高` 的像素值（如 `1024x1536`），**不要直接填比例文字**（如 `3:4`、`16:9`）——上游会报
> `size must be one of 'WIDTHxHEIGHT', '1k', '2k', or '4k'` 并拒绝出图（2026-07-03 生产实锤）。gpt-image-2
> （默认/推荐）官方保证 `1024x1024`(1:1) / `1024x1536`(2:3 竖) / `1536x1024`(3:2 横) / `auto`；**`1536x2048`
> (3:4 竖图，抖音/短视频封面常用) 已实测可用**，做视频封面图优先用它。旧文档的 `1792` 是 DALL·E-3 的档，
> 单独填可能被拒。要更精确的比例：先按最接近的档位出图，再裁剪/缩放。拿不准就用 `auto`。

输出（`--json`，stdout）：`{"ok":true,"file":"/abs/cat.png","model":"gpt-image-2","revised_prompt":"...","elapsed":"3.1s"}`

> 默认 `gpt-image-2`：最稳、改图/多图融合最强、直接回图（国内最可靠），一般无需换。`--model` 可透传任意模型 id（用户明确点名别的模型时才用），但别自作主张换——默认这个就够好。

### 图生图 / 参考图（场景手册）

带 `--ref`（可重复多张融合）就是拿参考图改图/合成。常见套路：

```bash
# 换背景：主体不变，只改背景
node scripts/gen-image.mjs --prompt "把背景换成纯白棚拍" --ref product.png --out product-white.png --json
# 改风格：照片转插画/国风等
node scripts/gen-image.mjs --prompt "改成扁平插画风格，保留人物特征" --ref photo.png --out illust.png --json
# 多图合成：把 A 的人物放进 B 的场景（多张 --ref）
node scripts/gen-image.mjs --prompt "把这个人物自然融入这个场景" --ref person.png --ref scene.png --out merged.png --json
# 局部精修 / 放大：提清晰度、补细节
node scripts/gen-image.mjs --prompt "提升清晰度和细节，主体构图保持不变" --ref small.png --out hd.png --json
```

`--ref` 用本地图片路径即可（脚本自动 multipart 上传，需系统 curl）。图生图走 `/v1/images/edits`，**不传 `response_format`**（脚本已处理）。

## 视频：scripts/gen-video.mjs

```bash
# 文生视频（默认 5s / 480p / mini 档）
node scripts/gen-video.mjs --prompt "橘猫在月球表面慢跑，星空背景" --out cat.mp4 --json

# 图生视频（首帧图），10 秒、720p、fast 档
node scripts/gen-video.mjs --prompt "让这张图里的猫动起来" --image cat.png \
  --model doubao-seedance-2-0-fast-260128 --duration 10 --resolution 720p --out cat-anim.mp4 --json
```

参数：`--prompt`(必填) `--model`(默认 `doubao-seedance-2-0-mini-260615`；三档见下表，文生/图生视频通用) `--image`(首帧图，可选，图生视频) `--duration`(秒，5~15，默认 5，服务器按此区间钳位) `--resolution`(`480p`默认 / `720p`（×1.5 价）/ `1080p`（更贵，上游 Seedance 已实测接受并出片）) `--out`(默认 ./uking-video-<时间>.mp4) `--retries`(上游明确失败且已退费后自动重试次数，默认 2) `--force-new`(明确要求同样参数再生成一版；默认会恢复未交付任务，避免重复扣费) `--key` `--json`

视频是异步任务，脚本自动**轮询**（每 5 秒，最多 20 分钟）+ **下载**。进度走 stderr，结果走 stdout。提交前会在 stderr 打一行**预计费用**（按 5s/480p 基准价 × 时长比例 × 分辨率系数估算，单位是人民币；余额接口字段虽叫 `hard_limit_usd`，U-King 视频额度里数值按人民币余额使用，**不要再乘美元汇率**）。

**中断恢复 / 防重复扣费**：脚本会在 POST 前先把幂等键写入 `~/.uking/video-jobs/`，拿到 `task_id` 后继续更新状态。电脑休眠、父批次被杀、终端断开或下载失败后，重新运行**同一条命令/同一份 jobs.json**即可恢复原任务并继续下载，不会重新生成、不会再扣一次。只有你明确想用完全相同的参数再生成一个不同版本时才加 `--force-new`。任务 20 分钟没回终态时只保留等待，绝不会把“还在跑”误判成失败后另开一条收费任务。

输出（`--json`）：`{"ok":true,"file":"/abs/cat.mp4","model":"...","task_id":"...","attempts":1,"elapsed":"68s"}`

> 上游偶发「跑到 100% 才判 failed」的间歇性失败（多为内容审核命中或临时波动，只回笼统 "task failed"）。脚本**默认自动重试 2 次**；若多次仍失败，会返回人话错误（建议换提示词重试）。要关掉重试加 `--retries 0`。

视频模型（字节跳动 Seedance，三档质量，价格是 5s/480p 基准价）：

| 模型 id | 定位 | 基准价（5s/480p） |
|---|---|---|
| `doubao-seedance-2-0-mini-260615`（**默认**） | 快·省，日常短视频/表情包首选 | ¥2.9 |
| `doubao-seedance-2-0-fast-260128` | 质量/价格平衡档 | ¥4.9 |
| `doubao-seedance-2-0-260128` | 最高清最精细，商用成片 | ¥6.9 |

> 时长按 5s 档线性加乘（10s = 基准价 ×2，15s = ×3），720p 分辨率再 ×1.5。例如 mini/10s/720p 实扣 ¥8.70，fast/15s/720p 实扣 ¥22.05；余额不足 ¥22.05 时后者会在建任务前拒绝且不扣费。旧模型名（`wanx2.1-t2v-turbo`/`dreamina-seedance-2-0`/`veo-3.1-*`）服务端仍会自动映射到对应档位兼容，但**新调用请直接用上表的新 id**，不要再当作可选项列出来。

## 🎬 一键成片：scripts/gen-reel.mjs（做「短视频/漫剧/宣传片」首选）

用户说「做个 XX 短视频 / 漫剧 / 宣传片」，**别自己一步步串 gen-image/gen-video/gen-stitch**——直接调 `gen-reel.mjs`，它一条命令把「分镜 → 出视频 →（可选配音）→ 拼接成片」全做完（**确定性编排 + 某镜头失败自动换档位/跳过**），比手动串稳得多、快得多。

```bash
# 内联分镜（--shot 可重复，格式 "画面::怎么动"，:: 后可省）
node scripts/gen-reel.mjs \
  --shot "赛博朋克未来城市夜景，霓虹，飞行汽车::镜头缓慢推进，霓虹闪烁" \
  --shot "发光的AI核心，蓝紫光，数据流环绕::核心旋转发光" \
  --resolution 720p --duration 5 --out reel.mp4 --json

# 分镜脚本文件（更细）
node scripts/gen-reel.mjs --storyboard sb.json --out reel.mp4 --json
# 加整段旁白 / 背景音乐
node scripts/gen-reel.mjs --shot "..." --narration "欢迎来到未来之城……" --voice nova --out reel.mp4 --json
```

参数：`--shot`(画面::运动，可重复) 或 `--storyboard`(sb.json) · `--resolution`(默认 720p) · `--duration`(每镜秒数，默认 5) · `--out` · `--i2v`(改图生视频：先出图当首帧，更可控/角色一致，但慢些) · `--ref`(角色参考图，隐含 i2v) · `--narration`+`--voice`(整段旁白) · `--bgm` · `--video-model`/`--image-model`/`--video-fallback`(兜底档位) · `--concurrency`(默认 2) · `--keep` · `--progress <file>`(进度落文件，便于监控) · `--json`

- **默认走文生视频**（快·稳，不用上传大图）；要**角色/画风一致的多镜头漫剧**加 `--i2v --ref 主角图.png`。
- 某镜头失败**自动换兜底档位**（fast→mini）再试，仍失败就跳过、用能成的镜头拼片（至少交付 1 段）。
- 输出（`--json`）：`{"ok":true,"file":"/abs/reel.mp4","shots":3,"clips":3,"resolution":"720p","elapsed":"..."}`
- sb.json：`{ style?, image_model?, video_model?, video_fallback?, resolution?, duration?, ref?, narration?, voice?, bgm?, out?, shots:[{image:"画面描述", motion?:"怎么动"}...] }`

## 批量 / 多进程：scripts/gen-batch.mjs

一次要出**多张图 / 多条视频**（多个提示词、多个镜头、批量配图），用它**并发**跑——每个任务一个独立子进程（真·多进程），带并发上限，比一条条 `gen-image`/`gen-video` 快很多。

```bash
# ① 同款批量：多个 --prompt（默认作图；加 --type video 则批量出视频）
node scripts/gen-batch.mjs --prompt "春" --prompt "夏" --prompt "秋" --prompt "冬" --concurrency 4 --json

# ② 精细批量：用 jobs.json 每条独立指定 type/model/参考图/输出名
node scripts/gen-batch.mjs --jobs jobs.json --concurrency 3 --json
```

`jobs.json` 是一个数组，每条一个任务：

```json
[
  { "type": "image", "prompt": "橘猫宇航员，电影感", "model": "gpt-image-2", "size": "1536x1024", "out": "cat.png" },
  { "type": "image", "prompt": "把背景换成星空", "ref": "cat.png", "out": "cat-space.png" },
  { "type": "video", "prompt": "橘猫在月球慢跑，星空背景", "out": "run.mp4" },
  { "type": "video", "prompt": "让这张图动起来", "image": "cat.png", "model": "doubao-seedance-2-0-fast-260128", "duration": 10, "out": "anim.mp4" }
]
```

参数：`--jobs`(jobs.json 路径) 或 `--prompt`(可重复) · `--type`(image|video，配合 --prompt) · `--model` · `--size`/`--n`/`--quality`(image) · `--duration`/`--resolution`(video) · `--concurrency`(默认 3) · `--outdir`(没写 --out 时的输出目录) · `--key` · `--json`

输出（`--json`）：`{"ok":true,"total":4,"succeeded":4,"failed":0,"outdir":"...","results":[{"idx":0,"ok":true,"file":"/abs/cat.png"},...]}`。任何单条失败 `ok=false` 且退出码 1，但其余任务照常完成。视频建议 `--concurrency` 不超过 4（每条都在烧额度）。

## 看「当前可用」模型（会更新，实时拉，不用改技能包）

> **选型看 [`MODELS.md`](./MODELS.md)**——全球最强模型的分档 + 「哪个活用哪个 + 挂了切谁」的替补链。本文讲怎么调，MODELS.md 讲调哪个。

虾盘云的模型是**动态的**（随时新增作图 / 视频 / 语音模型）。要拿最新清单，别只背本文——跑：

```bash
node scripts/list-models.mjs          # 人读：按 作图 / 视频 / 语音 / 对话 分组，★ 是推荐默认
node scripts/list-models.mjs --json   # 机读：{ok, models:{image,video,tts,asr,chat_count}, note}
```

它实时 `GET /v1/models`（虾盘云是 OpenAI 兼容网关，这就是官方标准模型清单）。**新模型一上线就自动出现在这里，无需更新本技能包、无需发版**。用户点名某个新模型（如更高清的作图 / 视频档）时，用 `--model <id>` 透传给 `gen-image.mjs` / `gen-video.mjs` 即可。

> **语音 / TTS / 配音**：已支持 `gpt-4o-mini-tts`（OpenAI `/v1/audio/speech` 格式），用 `scripts/gen-tts.mjs`（见上）。更多语音模型上线后会自动出现在 `list-models.mjs` 的「语音」组里。

## 文字转语音（TTS / 配音）：scripts/gen-tts.mjs

把文字合成语音（mp3），用于漫剧配音、短视频旁白、有声内容。中文发音自然。需系统 curl。

```bash
node scripts/gen-tts.mjs --text "欢迎来到虾盘云，这是一段配音。" --voice nova --out narration.mp3 --json
echo "很长的旁白也行……" | node scripts/gen-tts.mjs --out long.mp3   # 也可从 stdin 读，超长自动分段合成再拼接
```

参数：`--text`(要念的文字；也可位置参数 / stdin) `--voice`(音色：`alloy`默认/`echo`/`fable`/`onyx`/`nova`/`shimmer`) `--model`(默认 `gpt-4o-mini-tts`) `--speed`(0.25~4，语速) `--out`(默认 ./uking-tts-<时间>.mp3) `--key` `--json`。单次上限 4096 字，超长自动按句分段合成再拼接（有 ffmpeg 用它拼，否则二进制顺序拼）。

输出（`--json`）：`{"ok":true,"file":"/abs/narration.mp3","voice":"nova","chars":22,"elapsed":"2.5s"}`

> 音色都能念中文；`nova`/`shimmer` 偏女声、`onyx` 偏浑厚男声、`alloy` 中性。要不同角色配音就换 `--voice`。

## 视频拼接：scripts/gen-stitch.mjs（多镜头 / 漫剧成片）

把多段视频按顺序拼成一条成片。各段尺寸/帧率不同也能拼（自动缩放补边到统一分辨率）。**需系统 ffmpeg**（没有会给安装指引）。

```bash
# 顺序拼接多段（--in 可重复，或直接列文件）
node scripts/gen-stitch.mjs --in shot1.mp4 --in shot2.mp4 --in shot3.mp4 --out final.mp4 --resolution 1080p --json

# 拼接 + 盖一条背景音乐/配音（--audio，按视频总长截断）
node scripts/gen-stitch.mjs shot1.mp4 shot2.mp4 --audio bgm.mp3 --out final.mp4 --json
```

参数：`--in`(输入，可重复，≥2；也接受位置参数) `--out`(默认 ./uking-stitch-<时间>.mp4) `--resolution`(`480p`/`720p`默认/`1080p` 或 `宽x高`) `--audio`(可选，背景音/配音单轨) `--fps`(默认 30) `--json`。只拼视频轨（各段自带音轨忽略；要配音用 `--audio`）。

### 只给了小说 / 剧本 / 一句话点子？先拆分镜（漫剧最值钱的前半段）

用户往往**只给一段小说、剧本、或「做个 XX 的漫剧」**，没有现成分镜。你（agent）要先把它拆成结构化分镜，再走下面的出片流程。这半段拆解**是漫剧连贯不连贯的关键**，按这套来：

1. **拆幕拆镜**：先把故事按「起承转合」四幕拆开，每幕再拆 **3~6 个镜头**。一集漫剧通常 8~16 个镜头、每镜头 3~5 秒。
2. **建编号资产库（保一致的核心）**：先列全片固定元素并编号，之后每个镜头提示词都**引用同一套描述词**，人物才不会每镜头换脸：
   - 角色 `C1=（外貌/发型/服装/年龄，一句固定描述）`、`C2=…`
   - 场景 `S1=（地点/光线/风格）`、道具 `P1=…`
   - 统一**画风前缀**（如「日式二次元赛璐璐、暖色、电影感」）放每个提示词开头。
3. **先出角色参考图**：`gen-image.mjs --prompt "<画风前缀> C1 半身正面立绘" --out C1.png`。这张就是后面所有镜头的 `--ref`，锁住人物。
4. **每镜头写成一行**：`[镜号] 画面=<画风前缀 + C1/S1 + 构图> | 动作=<怎么动，图生视频用> | 时长=<秒> | 旁白=<台词/解说>`，整理成 `gen-batch.mjs` 的 `--jobs jobs.json`。
5. 然后走下面的「参考图 + 分镜脚本 → 成片」四步。

> 省工程量：全片统一用**一张主角参考图 `--ref`** + **统一画风前缀** + **首尾帧接力**（上一镜头尾帧当下一镜头首帧），就能在现有 Seedance（虾盘云已上架）上做出角色/画风连贯的多镜头漫剧，不必等专门的漫剧模型。

### 端到端：一张参考图 + 剧本 → 漫剧成片（组合本技能几个脚本）

用户给「参考图 + 分镜脚本」（或你上一步拆好的分镜）想要一条漫剧/短片，你这样串：

1. **二次生成分镜图**（图生图，保人物一致）：对每个镜头 `gen-image.mjs --prompt "<该镜头画面>" --ref 参考图.png --out shotN.png`（多镜头用 `gen-batch.mjs` 并发）。
2. **每个镜头出视频**（图生视频，用上一步的图当首帧）：`gen-video.mjs --prompt "<该镜头怎么动>" --image shotN.png --out shotN.mp4`（多镜头 `gen-batch.mjs --type video` 批量）。
3. **拼成成片**：`gen-stitch.mjs --in shot1.mp4 --in shot2.mp4 … --out 漫剧.mp4 --resolution 1080p`。
4. **配音/旁白**：`gen-tts.mjs --text "<该段旁白>" --voice nova --out vN.mp3` 合成每段配音（不同角色换 `--voice`），再 `gen-stitch.mjs … --audio 配音.mp3` 盖到成片上（或把多段旁白先拼成一条再盖）。纯背景音乐同理用 `--audio`。

## 让任意「OpenAI 兼容」的生图工具也走虾盘云

虾盘云是 **OpenAI 兼容网关**。任何用 OpenAI 图片接口、且**允许改 base_url** 的工具/skill（如 `openai-image-gen`、`image-generation`、各类 DALL·E 风格 skill），都可以不申请自己的 OpenAI key，直接指到虾盘云：

```bash
export OPENAI_BASE_URL=https://api.u-claw.org.cn/v1
export OPENAI_API_KEY=<你的设备 Key，见 ~/.uking/device.json 的 key 字段>
# 模型名用 gpt-image-2；不要用 dall-e-3（虾盘云无此名）
```

适用范围与注意：
- ✅ **作图**：标准 `/v1/images/generations`、`/v1/images/edits` 完全兼容，repoint 即用。
- ⚠️ **视频**：各家「OpenAI 视频」接口形态不统一，不保证兼容；**视频请优先用本技能的 `gen-video.mjs`**（已对齐虾盘云的异步契约，且知道怎么传 `duration`/`resolution`）。
- ❌ **厂商私有协议的 skill 无法只换 key**：可灵(`KLING_*`)、Nano Banana/Gemini(`GEMINI_API_KEY`)等走的是各自私有格式，端点也常写死——这类不必硬接，直接用本技能的 `gen-image.mjs`/`gen-video.mjs` 即可。

## 退出码

`0` 成功 · `1` 运行/网络错误 · `2` 参数错误。配合 `--json` 解析 stdout 的 `ok`/`file`/`error`。`gen-batch.mjs`：`0` 全部成功 · `1` 有失败。

## 纯 curl 用法（无 Node 时手调）

端点 `https://api.u-claw.org.cn`，鉴权 `Authorization: Bearer <KEY>`（KEY 见上「API Key」）。

### 文生图
```bash
curl -sS -m 600 -X POST https://api.u-claw.org.cn/v1/images/generations \
  -H "Authorization: Bearer $XIAPAN_API_KEY" -H "Content-Type: application/json" \
  -d '{"model":"gpt-image-2","prompt":"一只橘猫宇航员","n":1,"size":"1024x1024","response_format":"b64_json"}'
# 响应 data[0].b64_json 是 base64 PNG，base64 解码即图片。
# 若该模型只回 data[0].url，再下载（注意 --ssl-no-revoke）：
curl -sS -m 60 -L --ssl-no-revoke -o out.png "<那个 url>"
```

### 图生图（multipart，⚠️ 不要传 response_format）
```bash
curl -sS -m 600 -X POST https://api.u-claw.org.cn/v1/images/edits \
  -H "Authorization: Bearer $XIAPAN_API_KEY" \
  -F model=gpt-image-2 -F "prompt=把背景换成星空" -F n=1 -F size=1024x1024 -F image=@input.png
```

### 视频（提交 → 轮询 → 下载 三步）
```bash
# 1) 提交
curl -sS -m 60 -X POST https://api.u-claw.org.cn/v1/video/generations \
  -H "Authorization: Bearer $XIAPAN_API_KEY" -H "Content-Type: application/json" \
  -d '{"model":"doubao-seedance-2-0-mini-260615","prompt":"橘猫在月球慢跑","duration":5,"resolution":"480p"}'
# → {"task_id":"xxxxxx"}

# 2) 轮询（每 5 秒一次，直到 data.status 含 SUCCESS；含 FAIL/ERROR/CANCEL 则失败）
curl -sS -m 30 https://api.u-claw.org.cn/v1/video/generations/xxxxxx \
  -H "Authorization: Bearer $XIAPAN_API_KEY"
# → {"code":"success","data":{"status":"IN_PROGRESS|SUCCESS","progress":"50%","result_url":"https://api.u-claw.org/..."}}

# 3) 下载（把 result_url 的 api.u-claw.org 换成 api.u-claw.org.cn 再下）
curl -sS -m 300 -L --ssl-no-revoke -H "Authorization: Bearer $XIAPAN_API_KEY" -o out.mp4 "<result_url 改成 .org.cn>"
```

## 把本技能装进各 AI 工具

把整个 `uking-aigc/` 文件夹拷进对应目录，重启该工具即可：

- **Claude Code**：`~/.claude/skills/uking-aigc/`（官方 skills 目录，已验证可用）
- **OpenClaw / ClawX**：`~/.openclaw/skills/uking-aigc/`（便携版在 exe 同级 `data/.openclaw/skills/`，已验证可用）
- **Hermes**：`%LOCALAPPDATA%\hermes\skills\aigc\uking-aigc\`（**已真机验证**：pc-*** 实测 Hermes 会扫描该目录、识别 SKILL.md 并调用脚本，作图/TTS 都调通了）。
- **其它工具**：放到它的技能 / skills 目录即可，本技能是通用 SKILL.md 格式。
