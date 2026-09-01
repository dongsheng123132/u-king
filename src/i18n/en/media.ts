/** 英文覆盖字典 · AI 作图 / 视频 / 海报二维码（Draw/Video/QrMerge）。 */
export const media: Record<string, string> = {
  // ---- 共享 / 通用 ----
  "充值": "Top up",
  "清空": "Clear",
  "下载": "Download",
  "移除": "Remove",
  "已保存到：": "Saved to: ",
  "保存失败：": "Save failed: ",
  "读取图片失败": "Failed to read image",
  "读取图片失败：": "Failed to read image: ",
  "只能拖入图片文件": "Only image files can be dropped in",
  "未拿到图片": "No image returned",
  "生成": "Generate",
  "生成中": "Generating",
  "生成中…": "Generating…",
  "打包给 AI 用": "Package for AI",
  "把作图 / 视频能力打包成技能，复制说明或导出文件夹，给 OpenClaw / ClawX / Claude Code 等任意 AI 调用":
    "Package image / video generation into a skill — copy the guide or export a folder for OpenClaw / ClawX / Claude Code or any AI",

  // ---- AI 作图（Draw.tsx）----
  "AI 作图": "AI Image",
  "输入文字即画 · 拖入参考图可改图 · 用内置 Key 计费": "Type to draw · drag a reference to edit · billed with the built-in key",
  "松手添加参考图（图生图）": "Release to add a reference image (image-to-image)",
  "最多 {n} 张参考图": "Up to {n} reference images",
  "已加 {n} 张（上限 {max}）": "Added {n} ({max} max)",
  "请描述要怎么改这张图（例如：换成星空背景）": "Describe how to edit this image (e.g. change to a starry-sky background)",
  "请先输入要画的内容": "Enter what you want to draw first",

  // 作图路由 banner —— 只有把作图改到自家供应商的客户才看得到（默认走虾盘云时整条不渲染）
  "作图走 {name} · {model}（在「AI 设置 → 工具分配」里改）":
    "Images go through {name} · {model} (change it under AI Settings → Tools)",
  "作图走 {name}（在「AI 设置 → 工具分配」里改）":
    "Images go through {name} (change it under AI Settings → Tools)",
  "用这家自己的 Key 计费": "Billed with that provider's own key",

  // 报错分类给出的人话（lib/errorKind.ts）—— 这些不是软件 bug，不上报，直接告诉用户怎么办
  "额度不够了，去「虾盘云 · 充值」充一点就能继续。":
    "You're out of credit. Top up under \"Xiapan Cloud · Recharge\" and you can keep going.",
  "这个 Key 用不了（无效或还没建档）。去「虾盘云 · 充值」页确认一下 Key。":
    "This key isn't usable (invalid, or not registered on the server yet). Check it on the \"Xiapan Cloud · Recharge\" page.",
  "服务器这会儿忙不过来，等一两分钟再试。不是你电脑的问题。":
    "The server is overloaded right now — try again in a minute or two. It's not your computer.",
  "网络连不上服务器。检查一下网络，或换个网络（有的公司网/校园网会挡）。":
    "Can't reach the server. Check your connection, or try a different network (some office/campus networks block it).",
  "这次等太久超时了。图越复杂越慢，重试一次通常就好。":
    "This one timed out. More complex images take longer — retrying usually works.",
  "画图模型打不开网页，看不到这个网址里的任何内容——它只会照着域名编。想按官网内容出图，请把公司做什么、主色、要出现的文字直接写出来。（想分析网站请用「网站体检」页）":
    "The image model cannot open web pages — it sees nothing behind that URL and will simply invent something from the domain name. To match a real site, spell out what the company does, the brand colors and any text that must appear. (To analyze a site, use the Website Checkup page.)",
  "画图模型打不开网页，看不到那个网站的任何内容——它只会照着域名编。请直接描述你要的画面（想按官网内容出图，就把公司业务、主色、要出现的文字打出来）。再点一次即按原文作画。":
    "The image model cannot open web pages — it sees nothing on that site and will simply invent something from the domain name. Describe the image you want instead (spell out the company's business, brand colors and any text that must appear). Click again to draw your text as-is.",
  "{from} 这次没画成，已自动换 {to} 重画好了":
    "{from} couldn't draw this one; automatically redrawn with {to}",
  "出图成功": "Image generated",
  "出图失败：": "Image generation failed: ",
  "已清空作图记录": "Image history cleared",
  "描述你想要的画面，回车即画": "Describe the image you want, then press Enter to draw",
  "例如：一只穿宇航服的橘猫站在月球上，赛博朋克风，霓虹光":
    "e.g. an orange cat in a spacesuit standing on the moon, cyberpunk style, neon glow",
  "想改一张已有的图？把它拖进来当参考图": "Want to edit an existing image? Drag it in as a reference",
  "AI 出图": "AI-generated image",
  "点击放大": "Click to enlarge",
  "改写：": "Rewritten: ",
  // 出图失败后的建议（Draw.tsx::failureHint，按「哪种失败 + 哪个模型」分支给）
  "这张图出出来了，但存在境外图床、你的网络下载不回来。换成「GPT Image 2」重试——它直接返回图片，不走图床。":
    "The image was generated, but it lives on an overseas CDN your network can't reach. Retry with “GPT Image 2” — it returns the image directly instead of via a CDN.",
  "这是「GPT Image 2」的海外上游被限速了，不是你点太快，等多久都一样。点上面的模型下拉换成「Seedream 4.0」或「通义千问图片」（国产直连）就能出图。":
    "“GPT Image 2”'s overseas upstream is being rate-limited — this is not you clicking too fast, and waiting won't help. Use the model dropdown above to switch to “Seedream 4.0” or “Qwen Image” (direct domestic providers).",
  "这个模型的上游这会儿被限速了。点上面的模型下拉换一个模型再试。":
    "This model's upstream is currently rate-limited. Use the model dropdown above to pick another model and retry.",
  "可以点上面的模型下拉换一个模型再试（不同模型的上游是独立的，一个挂了另一个通常还在）。":
    "Try another model from the dropdown above — each model has an independent upstream, so if one is down the others usually still work.",
  "参考图": "Reference image",
  "正在按参考图改图…": "Editing based on the reference image…",
  "正在生成…": "Generating…",
  "已等待 {v}": "waited {v}",
  "切到别的页面也不会中断": "Switching to another page won't interrupt it",
  "文字多 / 要求细的图较慢，最长约 10 分钟，请耐心等":
    "Text-heavy / detailed images are slower, up to about 10 minutes; please be patient",
  "参考图（图生图）：": "Reference image (image-to-image): ",
  "添加参考图（也可直接拖入或粘贴）": "Add a reference image (or drag / paste directly)",
  "描述要怎么改这张图（回车发送）": "Describe how to edit this image (press Enter to send)",
  "描述要画的画面，回车即画（Shift+Enter 换行；可拖/粘贴参考图）":
    "Describe the image, press Enter to draw (Shift+Enter for a new line; drag/paste a reference)",
  "作图模型（看优缺点自己选；图生图建议用 GPT Image 2）":
    "Image model (pick by pros and cons; GPT Image 2 recommended for image-to-image)",
  "出图尺寸（可手填任意尺寸；拿不准就选「自动」）": "Image size (type any size; choose “Auto” if unsure)",
  "手填尺寸，如 1024x1536，回车确认": "Type a size, e.g. 1024x1536, press Enter to confirm",
  "出图中": "Generating",
  "改图": "Edit",

  // ---- AI 海报二维码（QrMerge.tsx）----
  "AI 海报二维码": "AI Poster QR",
  "AI 出图配的假二维码扫不出？拿真二维码换上去，拖一拖、合成导出":
    "Can't scan the fake QR from AI art? Swap in a real QR code, drag it around, then composite and export",
  "图片解析失败，请换一张": "Failed to parse image, please try another",
  "检测中…": "Checking…",
  "已验证可扫": "Verified scannable",
  "本地快速自检，不代表 100% 准确；实际能不能扫建议用手机再确认一次":
    "Local quick self-check, not 100% accurate; please confirm with your phone whether it actually scans",
  "未检测到，试试调大二维码或减少旋转角度": "Not detected — try enlarging the QR code or reducing the rotation",
  "只能选择图片文件": "Only image files can be selected",
  "裁剪失败，请重试": "Crop failed, please try again",
  "背景图生成成功": "Background image generated",
  "生成失败：": "Generation failed: ",
  "读取作图历史失败：": "Failed to read image history: ",
  "请输入网址或文字": "Enter a URL or text",
  "二维码已生成": "QR code generated",
  "生成二维码失败：": "Failed to generate QR code: ",
  "先放好背景图和二维码": "Add a background image and a QR code first",
  "合成失败": "Compositing failed",
  "导出失败：": "Export failed: ",
  "背景图": "Background",
  "背景预览": "Background preview",
  "换一张": "Replace",
  "上传": "Upload",
  "AI 生成": "AI generate",
  "最近作图": "Recent images",
  "点击选择 / 拖入 / 粘贴一张背景图": "Click to choose / drag / paste a background image",
  "描述海报内容，比如：简约风格的加好友海报，浅色背景，留出下方空间":
    "Describe the poster, e.g. a minimalist add-friend poster, light background, leave space at the bottom",
  "作图模型": "Image model",
  "出图尺寸": "Image size",
  "手填尺寸，回车确认": "Type a size, press Enter to confirm",
  "生成背景图": "Generate background",
  "读取中…": "Loading…",
  "还没有作图记录，先去「AI 作图」画一张": "No images yet — go to “AI Image” and draw one first",
  "历史作图": "Past image",
  "真二维码": "Real QR code",
  "二维码预览": "QR code preview",
  "重新裁剪": "Re-crop",
  "裁剪": "Crop",
  "换一个": "Replace",
  "上传截图": "Upload screenshot",
  "网址/文字生成": "From URL/text",
  "点击选择 / 拖入 / 粘贴你的二维码截图": "Click to choose / drag / paste your QR code screenshot",
  "微信/支付宝「扫我加好友」二维码是平台颁发的，不能靠文字生成，请上传截图。":
    "WeChat/Alipay “scan to add me” QR codes are issued by the platform and can't be generated from text — please upload a screenshot.",
  "输入网址或文字，比如 https://example.com": "Enter a URL or text, e.g. https://example.com",
  "仅适用于普通网址/文字二维码；微信/支付宝二维码请用「上传截图」。":
    "Only for plain URL/text QR codes; for WeChat/Alipay QR codes use “Upload screenshot”.",
  "生成二维码": "Generate QR code",
  "调整": "Adjust",
  "大小": "Size",
  "旋转": "Rotation",
  "上移": "Move up",
  "左移": "Move left",
  "重置位置": "Reset position",
  "右移": "Move right",
  "下移": "Move down",
  "自动加白边留白（推荐开启，更容易扫出）": "Auto-add a white quiet zone (recommended, easier to scan)",
  "先在左边选一张背景图": "First pick a background image on the left",
  "上传已有的海报，或用 AI 现场生成一张": "Upload an existing poster, or generate one with AI on the spot",
  "导出中": "Exporting",
  "导出成品图": "Export final image",
  "再在左边放一个真二维码，就能拖到图上了": "Add a real QR code on the left, then you can drag it onto the image",
  "框选二维码所在区域，去掉多余的边框/文字/头像": "Select the QR code area, removing extra borders/text/avatars",
  "裁剪二维码": "Crop QR code",
  "取消": "Cancel",
  "跳过裁剪，直接用原图": "Skip cropping, use the original",
  "确认裁剪": "Confirm crop",

  // ---- AI 视频（Video.tsx）----
  "AI 视频": "AI Video",
  "视频片段": "Video Clip",
  "文字生成视频 · 拖入首帧图可图生视频 · 异步出片约 1～3 分钟":
    "Text to video · drag in a first frame for image-to-video · async rendering, about 1–3 min",
  "松手设为首帧图（图生视频）": "Release to set as the first frame (image-to-video)",
  "已切到{name}做图生视频（所选模型不支持首帧图）":
    "Switched to {name} for image-to-video (the selected model doesn't support a first frame)",
  "只能用图片当首帧": "Only an image can be used as the first frame",
  "请先输入要生成的视频画面": "Enter the video scene you want to generate first",
  "上传首帧图并提交中…（图生视频较慢，请耐心等 1–3 分钟，勿重复点）":
    "Uploading first frame and submitting… (image-to-video is slow, please wait 1–3 minutes, don't click repeatedly)",
  "提交中…": "Submitting…",
  "视频生成完成": "Video generated",
  "正在继续原任务，只重试查询和下载，不会重新生成或扣费":
    "Resuming the original task — only status checking and download are retried; it will not regenerate or charge again",
  "视频已恢复并下载完成": "Video recovered and downloaded",
  "原任务暂时还没下载下来：": "The original task still could not be downloaded: ",
  "视频已经生成，正在等待下载到本机": "The video is ready and waiting to download to this computer",
  "重试下载（不重新扣费）": "Retry download (no new charge)",
  "视频生成失败：": "Video generation failed: ",
  "生成中 {pct}": "Generating {pct}",
  "读取视频失败：": "Failed to read video: ",
  "已清空视频记录": "Video history cleared",
  "描述你想要的视频画面，回车即生成": "Describe the video you want, then press Enter to generate",
  "例如：一只橘猫宇航员在月球上慢跑，电影感，星空背景":
    "e.g. an orange-cat astronaut jogging on the moon, cinematic, starry-sky background",
  "视频较慢，出片约 1～3 分钟，切走也不中断": "Video is slow, rendering takes about 1–3 min, and won't stop if you switch away",
  "播放视频": "Play video",
  "生成中…（重开后已自动续跑，约 1～3 分钟）": "Generating… (auto-resumed after restart, about 1–3 min)",
  "未知错误": "Unknown error",
  "首帧": "First frame",
  "视频较慢，切到别的页面也不会中断": "Video is slow; switching to another page won't interrupt it",
  "首帧图（图生视频）：": "First frame (image-to-video): ",
  "设首帧图做图生视频（也可拖入或粘贴）": "Set a first frame for image-to-video (or drag / paste)",
  "描述首帧怎么动（回车生成图生视频）": "Describe how the first frame should move (press Enter for image-to-video)",
  "描述要生成的视频画面，回车即生成（可拖/粘贴首帧图）":
    "Describe the video, press Enter to generate (drag/paste a first frame)",
  "视频模型（看优缺点自己选）": "Video model (pick by pros and cons)",
  "预估约 ¥{price} / 5 秒（按 480p，实扣以服务器为准）":
    "Estimated ¥{price} / 5 sec (480p reference; final charge is set by the server)",
  "图生视频": "Image-to-video",
  "生成视频": "Generate video",
  // ---- AI 作图：迭代按钮 / 场景预设引导 / 画质档（本轮新增） ----
  "需图": "needs img",
  "用它改图": "Edit this",
  "再画一版": "Regenerate",
  "在这张的基础上改：把它设为参考图，再描述要怎么改":
    "Edit from this image: set it as the reference, then describe the changes",
  "用同样的提示词再画一版（可先改几个字）":
    "Regenerate with the same prompt (tweak a few words first if you like)",
  "复制这条提示词": "Copy this prompt",
  "已填回提示词，改几个字或直接点生成": "Prompt filled back in — tweak a few words or just hit Generate",
  "已把这张设为参考图，描述要怎么改（如：换成星空背景、衣服改红色）":
    "Set this as the reference image — describe the changes (e.g. starry-sky background, red clothes)",
  "提示词已复制": "Prompt copied",
  "复制失败，请手动选中复制": "Copy failed — please select and copy manually",
  "↓ 不知道画什么？点下方「场景模板」一键套用专业提示词":
    "↓ Not sure what to draw? Tap a scene template below for a one-click pro prompt",
  "知道了": "Got it",
  "已选「{label}」——这是改图玩法：先点左下的 🖼 或把一张照片拖 / 粘进来，再点生成":
    "Selected “{label}” — this edits an image: tap 🖼 at bottom-left or drag / paste a photo in, then Generate",
  "已套用「{label}」——把提示词里「」括起来的地方换成你自己的内容，再点生成":
    "Applied “{label}” — replace the text inside 「」 with your own, then Generate",
  "已套用「{label}」——可直接点生成，或改几个字再生成":
    "Applied “{label}” — hit Generate, or tweak a few words first",
  "画质：标准省钱；高清更精细，但约 4 倍价钱":
    "Quality: Standard saves money; HD is finer but ~4× the cost",
  "标准": "Standard",
  "高清": "HD",
  "已切高清：更精细，但约 4 倍价钱；通用配图用标准即可":
    "Switched to HD: finer, but ~4× the cost; Standard is fine for everyday images",
};
