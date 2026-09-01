/**
 * 虾盘云可选模型清单 —— Manager「换模型」下拉 + MyAI/ToolAppView 的 ProviderSwitch 共用。
 * 单一来源，改一处全生效。
 */
/** 单个模型项。
 *  - label：下拉里显示的名字（含一句话定位）
 *  - desc：给小白看的「人话说明」——它擅长啥、什么时候选它，下拉里以小字展示
 *  - recommend：是否打「推荐」标，给没主意的客户指一条默认路 */
export type ModelItem = { id: string; label: string; desc?: string; recommend?: boolean };
export type ModelGroup = { group: string; items: ModelItem[] };

/** ⚠️ 改这里的 id 前先真调一次 `/v1/chat/completions`（最后一次全量核对：2026-08-01）。
 *  虾盘云 `/v1/models` 会同时列**裸名**（直连渠道）和 `anthropic/` `openai/` 这类**中转前缀名**，
 *  两者不等价：实测 `claude-opus-5` 裸名 503（没建直连渠道）、`anthropic/claude-opus-5` 才 200。
 *  这份清单只收裸名 —— 前缀名走的是中转商，中转商一欠费就整片 403（见 memory
 *  `xiapan-easyrouter-drain-openrouter-failover`），不该拿它当客户的默认推荐。 */
export const XIAPAN_MODELS: ModelGroup[] = [
  {
    group: "性价比之选",
    items: [
      // ⚠️ flash 必须在列表里、必须是推荐项 —— 它就是 providers.rs 虾盘云 preset 的默认
      // 模型（开箱即用的那个）。这里曾经**只列 pro 不列 flash**，还把 pro 标成「最快最省」：
      // 客户实际在用 flash，可下拉里找不到它，唯一带★的又是更贵的 pro，随手点一下就从
      // 便宜换成贵的 —— 而 pro 是推理模型，输出预算一紧就把 token 烧在 reasoning 上、
      // 正文返回空（2026-07-10 一台 Mac 客户机实锤「无法生成回复，请重试」）。
      {
        id: "deepseek-v4-flash",
        label: "DeepSeek V4 Flash · 最快最省（默认）",
        desc: "不知道选啥就用它，装好就是它。国产、速度快、最省额度，日常聊天问答写文章都够用。",
        recommend: true,
      },
      {
        id: "deepseek-v4-pro",
        label: "DeepSeek V4 Pro · 满血推理",
        desc: "同门的「深思」版，遇到难题会先想一大段再答，复杂推理更强，但更慢也更费额度。日常别用它，卡住了再切。",
      },
      {
        id: "kimi-k3",
        label: "Kimi K3 · 超长上下文",
        desc: "能一口气读很长的文档/几十页 PDF，做长文总结、整本资料分析时选它。月之暗面最新一代。",
      },
      {
        id: "MiniMax-M3",
        label: "MiniMax M3 · 海螺",
        desc: "国产通用模型，写作、对话表现均衡，可作 DeepSeek 之外的备选。",
      },
      {
        id: "glm-5.2",
        label: "GLM-5.2 · 智谱",
        desc: "清华系国产模型，中文理解到位，写报告、改公文比较顺手。",
      },
      {
        id: "qwen3.7-max",
        label: "Qwen3.7 Max · 通义",
        desc: "阿里通义旗舰，中文和代码都强，国产里偏「聪明」的一档。注意它只吃文字、不收图，要发图请用下面的看图模型。",
      },
      {
        id: "gemini-3.5-flash",
        label: "Gemini 3.5 Flash · 极速",
        desc: "谷歌的轻快版，回答又快又便宜，适合简单问答、批量处理。",
      },
    ],
  },
  /** ⚠️ 这一组的★曾经挂在 `qwen-vl-max` 上、文案写「看图最准」——**跟我们自己的跑道反着**。
   *  `skills/vision/scripts/bench.mjs`（三类合成夹具、带 ground truth）实测合计命中率：
   *    qwen3.7-flash 100% · qwen3-vl-flash 97% · qwen3.7-plus 95% · qwen3.6-flash 92% ·
   *    qwen-vl-max 91% · qwen-vl-plus 86% · qwen3.5-ocr 69% · MiniMax-M3 66%
   *  长截图（2400×2908）一项差距最大：qwen3.7-flash 4/4，而 **qwen-vl-max 只有 13%**。
   *  ——「名字里带 max 就最强」是错的，★ 已按数字移到 qwen3.7-flash。改这一组前先跑一遍 bench。
   *
   *  🔴 同样重要：**别把「plus」当成「能看图」的标志**。虾盘云上 `qwen-plus` / `qwen-turbo` /
   *  `qwen3.7-max` 都是纯文本，且发图给它们**不报错**——会返回一个编出来的答案
   *  （2026-08-16 实测：问执照上的法定代表人，正解「张示例」，两个都答「张三」）。
   *  所以纯文本模型一律不进这一组，`qwen3.7-max` 的 desc 里也明写了它不收图。 */
  {
    group: "📷 看图识图（能读图片 / 截图 / CAD 图纸）",
    items: [
      {
        id: "qwen3.7-flash",
        label: "通义 Qwen3.7 Flash · 看图最准（默认）",
        desc: "既能聊天又能「看懂」图片——照片、截图、CAD 图纸、表格、票据都认得，长截图也不漏。发图就选它，还最省额度。（DeepSeek 等纯文本模型看不了图）",
        recommend: true,
      },
      {
        id: "qwen3.7-plus",
        label: "通义 Qwen3.7 Plus · 看图更能想",
        desc: "同代的加强版，看得准之外更会分析推理，复杂图表 / 需要动脑的图交给它。比 Flash 慢一倍、贵一些。",
      },
      {
        id: "qwen3-vl-plus",
        label: "通义 Qwen3-VL Plus · 专职识图",
        desc: "专做识图的一支，逐字转录整页文档时输出更省。（2026-08-01 真图实测通过）",
      },
      {
        id: "qwen-vl-max",
        label: "通义 Qwen-VL Max · 老版识图",
        desc: "上一代识图模型，普通图片够用；但很长的截图容易漏读，长图请用上面的 Qwen3.7 Flash。",
      },
    ],
  },
  {
    group: "全球旗舰（更聪明，更费额度）",
    items: [
      {
        id: "claude-sonnet-5",
        label: "Claude Sonnet 5 · 编程之王",
        desc: "写代码、改 bug 最强的一档，程序员首选。比国产费额度。",
      },
      {
        id: "claude-opus-4-8",
        label: "Claude Opus 4.8 · 顶配旗舰",
        desc: "最聪明、最会思考，复杂分析/难题/大工程交给它。也最费额度。（Opus 5 目前只有中转渠道、裸名调不通，所以这里仍推 4.8）",
        recommend: true,
      },
      {
        id: "claude-opus-4-7",
        label: "Claude Opus 4.7 · 强力推理",
        desc: "比 4.8 稍早一代，依然很聪明，费额度比 4.8 略低。",
      },
      {
        id: "claude-haiku-4-5",
        label: "Claude Haiku 4.5 · 轻快",
        desc: "Claude 家的快版，比 Sonnet 便宜，日常用又快又稳。",
      },
      {
        id: "gpt-5.4",
        label: "GPT-5.4 · OpenAI 旗舰",
        desc: "ChatGPT 同款旗舰，综合能力强、知识面广，什么都能聊。",
      },
      {
        id: "gpt-5.4-mini",
        label: "GPT-5.4 Mini",
        desc: "GPT 的小号，便宜不少，简单任务用它省额度。",
      },
      {
        id: "gemini-3.1-pro-preview",
        label: "Gemini 3.1 Pro",
        desc: "谷歌最强旗舰，长文档和多模态理解好，知识更新快。",
      },
      {
        id: "grok-4.5",
        label: "Grok 4.5 · xAI",
        desc: "马斯克 xAI 的模型，风格直接、时事跟得紧，想换口味可以试。",
      },
    ],
  },
];

/** 「一键切看图模型」该切到哪个 —— 从上面看图组里取带★的那条。
 *
 *  🔴 **界面上别再写第二个 id 出来。** ProviderSwitch 那个「发图识别引导」按钮原本硬编码
 *  `qwen-vl-max`：等这份清单的★按跑道数据换掉之后，按钮还在继续把客户切到旧模型，
 *  而下拉里显示的推荐是另一个 —— 两处说法不一致，且只看界面永远查不出（同类事故 pc-***）。
 *  取不到就返回 null，调用方该显式处理，不许再兜一个字面量上去。 */
/**
 * 把**本地清单**和**服务端 `/v1/models` 实际有的**合到一起。
 *
 * ## 为什么要有它（2026-08-20 实测的账）
 * 服务端当时 39 个模型（对话类 21），而这份手工清单最后一次全量核对是 **2026-08-01**。
 * 结果：**13 个服务端有、清单没有** —— `kimi-k2.6` / `glm-5` / `glm-5.1` /
 * `qwen3.6-plus` / `deepseek-reasoner` / `deepseek-chat` … 客户在下拉里**根本看不到**。
 *
 * ★ 客户问「充值就能用吧？新出的 deepseek v5 呢？」——
 *   **对服务端成立，对界面不成立**：额度是够的、渠道也开了，但**下拉里没有就等于不存在**
 *   （没人会去手填模型 id）。按老做法，新模型要等我们改代码 + 发版才轮得到客户。
 *
 * ## 职责怎么分（这是关键，不是布局问题）
 *   · **`/v1/models` 是「有没有」的唯一真相源** —— 它说有才有。
 *   · **本地清单降级成「说明书」** —— 只负责 label / desc / 推荐标 / 分组顺序。
 *
 * 于是三种情形各有各的对法：
 *   | 服务端 | 清单 | 结果 |
 *   |---|---|---|
 *   | 有 | 有 | 显示，带人话说明（最好的情况） |
 *   | 有 | 没有 | **照样能选**，显示裸 id，归到「服务端还有这些」组 —— 新模型当天可用，不用发版 |
 *   | 没有 | 有 | **不显示** —— 别让客户点到一个 503 |
 *
 * ## 🔴 拉不到的时候必须退回清单，不能显示空
 * `live` 为空 = **不知道**服务端有什么（离线 / 超时 / key 没配好），不等于「什么都没有」。
 * 这时原样返回本地清单 —— 宁可多显示几个，也不能让客户打开下拉发现一片空白、
 * 以为产品坏了。★ 同 readiness 那条：不知道就说不知道，别把「没测到」渲染成「没有」。
 */
export function mergeModels(live: string[] | null | undefined): ModelGroup[] {
  if (!live || live.length === 0) return XIAPAN_MODELS; // 不知道 ≠ 没有
  const liveSet = new Set(live);
  const known = new Set<string>();
  const groups: ModelGroup[] = [];
  for (const g of XIAPAN_MODELS) {
    const items = g.items.filter((m) => {
      known.add(m.id);
      return liveSet.has(m.id);
    });
    if (items.length) groups.push({ group: g.group, items });
  }
  // 服务端有、清单没收的 —— 不藏起来，但也不假装懂它，只给裸 id。
  // 过滤掉明显不是对话模型的（作图 / 视频 / 语音 / OCR），那些有各自的入口，
  // 混进「换模型」下拉只会让人选错。
  const extra = live
    .filter((id) => !known.has(id))
    .filter((id) => !/image|seedance|seedream|wanx|wan2|dreamina|tts|ocr|-vl-/i.test(id))
    .sort();
  if (extra.length) {
    groups.push({
      group: "服务端还有这些（新上的，我们还没写说明）",
      items: extra.map((id) => ({ id, label: id })),
    });
  }
  return groups;
}

export function recommendedVisionModel(): ModelItem | null {
  const g = XIAPAN_MODELS.find((x) => x.group.startsWith("📷"));
  if (!g || !g.items.length) return null;
  return g.items.find((m) => m.recommend) || g.items[0];
}

/**
 * 「贵/慎用模型」成本提示 —— 单一来源，Manager 换模型框 + 其它选模型处共用。
 *
 * 背景（2026-07-22 实锤）：客户在「换模型」里**手填 / 🔄 拉取全部**选到了 `gpt-5.6-sol`
 * 这类海外前沿旗舰（不在精选下拉里、精选列表管不住这条路），满上下文单次可扣掉几块钱、
 * 一天烧掉 ¥169，把余额打穿后来问「充值的 token 是不是不见了」（其实是被这模型花光了）。
 *
 * 返回非 null = 该在 UI 上弹一条 amber 成本警告。**只提醒、不拦截**（对齐「手填永远可用 /
 * 健壮优先」的设计取舍）——客户仍可选，但先知道这是「现金老虎」。命中面故意收窄到真正
 * 危险的前沿系（gpt-5.5+/5.6、o 系推理），避免对 claude-sonnet 等正当编程选择过度打扰。
 */
/**
 * 「Codex 该填哪个模型」提示 —— **单一真相源**，Manager 换模型框 + ProviderSwitch 横幅共用。
 *
 * 🔴 为什么非得收成一份：这句话此前有两份，且已经漂到互相矛盾 ——
 * `ProviderSwitch.tsx` 2026-08-11 已按供应商分过文案（DeepSeek 官方裸名是**对的**，
 * 客户机 pc-*** 用他自己的 Key 实测 200），`Manager.tsx` 那份却还在无差别地对
 * **所有**供应商喊「换成国产裸名会报 not implemented」。对用 DeepSeek 官方的客户，
 * 那是在劝他别用唯一能用的模型 —— 比不提示更糟（宪法第 8 条：同一事实存在几份就漂几份）。
 *
 * 返回 null = 这个供应商没有 Codex 专属坑，别弹（少一条噪音，剩下的才有人读）。
 *
 * ## gpt-5.3-codex 到底贵几倍：老 UI 文案的「约 6 倍」是错的
 *
 * 三处各写各的：UI「约 6 倍」、`providers.rs:14` 与 `lib.rs:3621` 都写「贵几十倍」。
 * 按 `.tmp-er_pricing.json`（上游价目缓存，2026-06-22）实算：
 *
 * |                    | model_ratio | completion_ratio | 输出实际倍率 |
 * |--------------------|------------|------------------|-------------|
 * | deepseek-v4-flash  | 0.07       | 2                | 0.14        |
 * | gpt-5.3-codex      | 0.875      | 8                | 7.0         |
 *
 * → **输入 12.5 倍、输出 50 倍**。「6 倍」连输入那一头都不够，是三处里唯一错的一处，
 * 而它恰好是**唯一印在客户眼前**的那处。「几十倍」的说法成立，两条注释不用改。
 *
 * ⚠️ 该表是上游中转商的缓存、不是虾盘云自己的实时价目，所以文案只说量级不报具体倍数。
 * 要报死数字：查 new-api 后台这两个模型的 model_ratio / completion_ratio 现值。
 */
export function codexProtocolHint(p: { id: string; builtin_recharge?: boolean }): string | null {
  // 虾盘云内置：能不能用取决于模型挂在哪条渠道上，不是「国产都不行」
  if (p.builtin_recharge || p.id === "xiapan") {
    return "Codex 只认 Responses 协议。虾盘云请用 deepseek-v4-flash-codex（默认，已验证，跟 deepseek-v4-flash 同价）—— 填裸名 deepseek-v4-flash 会报 500 convert_request_failed。想更强可选 gpt-5.3-codex，但它贵十几到几十倍（输出那头差得最狠），别当日常。";
  }
  // DeepSeek 官方 2026-07-31 上线 V4-Flash 正式版，原生支持 Responses，裸名就是对的
  if (p.id === "deepseek") {
    return "Codex 只认 Responses 协议。DeepSeek 官方请用裸名 deepseek-v4-flash（已验证）—— deepseek-v4-pro 官方还没开放 Codex，选了会被直接拒绝。";
  }
  // GLM / Kimi 官方没有 /v1/responses，U-King 只能退回 chat 协议（见 providers.rs 模块注释）
  if (p.id === "glm" || p.id === "kimi") {
    return "GLM / Kimi 官方没有 Responses 接口，U-King 只能按老的 chat 协议配 —— 只有 0.8x 老版 Codex 认。要用新版 Codex，请换虾盘云或 DeepSeek 官方。";
  }
  return null;
}

export function priceyModelHint(id: string): string | null {
  const m = (id || "").trim().toLowerCase();
  if (!m) return null;
  // gpt-5.5 / 5.6 前沿系（sol / terra / luna …）—— 最烧钱的一档，正是把客户余额打穿的元凶
  if (/^gpt-5\.[5-9]/.test(m) && !m.includes("mini")) {
    return "⚠️ 海外前沿旗舰，最费额度 —— 满上下文单次可能扣掉几块钱（约国产 DeepSeek 的几十倍），一天能烧掉上百元。日常强烈建议用 DeepSeek / 国产模型，确有需要再用它。";
  }
  // o 系推理模型（o1/o3/o4）也偏贵，给同款提示
  if (/^o[1-9]([.-]|$)/.test(m)) {
    return "⚠️ 海外推理模型，费额度较高。日常任务用 DeepSeek / 国产更省，确需强推理再用。";
  }
  return null;
}

/** 作图模型 —— gpt-image-2 仍是默认与首选，但**必须留国产逃生口**。
 *
 *  2026-07 那版把列表砍到只剩 gpt-image-2（理由是省掉选择焦虑 + 清掉 wanx 尺寸越界的坑），
 *  代价在 2026-07-28 兑现了：gpt-image-2 的上游中转商出口 IP 被 Cloudflare 限速（error 1015），
 *  全量 429 打不出图；而 `Draw.tsx` 是 `IMAGE_MODELS.length > 1` 才显示下拉，于是客户在界面上
 *  **连换个模型的地方都找不到**，报错还劝他「建议用 GPT Image 2」——他正用着它。
 *  单一供应商 + 藏起选择器 = 上游一挂就是全量事故，这条别再回退。
 *
 *  只收**全尺寸实测通过**的（2026-07-28 首次逐档验过 IMAGE_SIZES 的 4 个固定档，2026-08-01 复验）：
 *    · qwen-image-2.0    4/4 通过（2026-08-01 实测；接替 qwen-image）
 *    · seedream-4-0      4/4 通过
 *    · wanx2.1-t2i-turbo ✗ 非方图直接 `Either width or height should be between 512 and 1440`
 *      —— 就是当年那个「选了小众模型才踩」的坑，所以**故意不收**。加新模型前先跑一遍全尺寸。
 *    · seedream-4-5      ✗ **0/4，四个尺寸全 `upstream_rejected`**（2026-08-01 实测）。它在
 *      `/v1/models` 里**列着**但服务端没有能出图的渠道 —— 「模型清单里有」≠「调得通」，
 *      照着上游发布稿把版本号 +0.5 就是全量出不了图。这条正是「加新模型前先跑全尺寸」的由来。
 *
 *  ★ 排序按**供应商独立性**，不是按画质。查虾盘云 abilities 路由表发现：
 *    gpt-image-2 与 seedream-4-0 走同一家上游（历史事故：同源双挂），
 *    只有 qwen 系是阿里云百炼国内直连。所以备胎第一位放 qwen —— 中转商整体出事时
 *    （2026-07-28 就是这个形态）它是唯一还站着的那条腿。后端自动兜底同理，见
 *    `providers.rs::OUTAGE_FALLBACK_GEN_MODEL`。
 *
 *  ⚠️ 价格不在前端展示、也不在此维护：实扣由服务器 new-api ModelPrice 决定（随上游浮动）。
 *  这样改服务器定价无需客户端发版，也不会出现「显示价≠实扣价」。成本/定价见 memory: xiapan-pricing-audit-easyrouter-usd-trap。 */
export const IMAGE_MODELS: {
  id: string;
  label: string;
  desc?: string;
  recommend?: boolean;
  /** 支不支持「图生图 / 改图」（`/v1/images/edits` 端点）。false 的模型在挂了参考图时不出现在下拉里，
   *  免得客户选中后收到一句看不懂的上游报错。缺省视为 true。 */
  edits?: boolean;
}[] = [
  {
    id: "gpt-image-2",
    label: "GPT Image 2 · OpenAI",
    desc: "最稳定、最听话，改图 / 多图融合最强，标准图还最便宜。",
    recommend: true,
    edits: true,
  },
  {
    id: "qwen-image-2.0",
    label: "通义千问图片 2.0 · 阿里",
    desc: "最稳的备胎：阿里国内直连（不过任何中转商），图存国内 OSS 下载最快，中文字体表现好。只能画新图，不能改图。",
    edits: false,
  },
  {
    id: "seedream-4-0",
    label: "Seedream 4.0 · 字节",
    desc: "画风讨喜、中文听得懂，改图也支持。审核比较宽松，真人照片改图被拒时用它。",
    edits: true,
  },
];

/** AI 作图「场景预设」—— 点一下自动填**专业提示词** + 建议尺寸，解决小白最大痛点「不会写提示词」。
 *  提示词取自圈内通用公式（awesome-gpt-image-2 / 电商提示词库）。改这里内容**不动后端**。
 *  `needRef=true` 的是「图生图 / 改图」预设：要先拖 / 粘一张参考图（走 edits 端点，产品/人物不被 AI 篡改）。
 *  形态对齐 Fooocus「一键 style 预设」——点卡片 → 提示词+尺寸自动填好，用户只改几个空。 */
export type DrawPreset = { id: string; cat: string; emoji: string; label: string; prompt: string; size?: string; needRef?: boolean; hint?: string };
/** 分类标签（UI 顺序）。预设一多按类切，不然一排 chip 太长。 */
export const DRAW_PRESET_CATS = ["🔥 爆款", "🛒 商用", "🧑 人像", "🛠 实用"] as const;
export const DRAW_PRESETS: DrawPreset[] = [
  // ——— 🔥 爆款传播（会自发分享，排最前面拉新） ———
  { id: "figure", cat: "🔥 爆款", emoji: "🧸", label: "3D 手办", needRef: true, size: "1024x1536",
    prompt: "将这张照片变成一个精致的角色手办。手办放在简洁的圆形透明底座上，背景是干净整洁的桌面，手办后面放一个印有该角色形象的商品包装盒，旁边一台电脑屏幕显示 Blender 建模过程。保持面部特征、发型和服装与原照一致，柔和的摄影棚灯光，收藏级 PVC 手办质感，逼真。" },
  { id: "lego", cat: "🔥 爆款", emoji: "🧱", label: "乐高小人", needRef: true, size: "1024x1024",
    prompt: "将照片中的人物转化为乐高小人包装盒的风格，等距透视呈现。包装盒上标注标题「你的名字」，盒内展示基于照片人物的乐高小人，并配上他必需的物品（如咖啡杯/背包/手机）作为乐高配件。盒子旁边再展示一个已拆封、真实立体渲染的乐高小人本体，逼真生动。" },
  { id: "sticker", cat: "🔥 爆款", emoji: "😄", label: "表情包/贴纸", needRef: true, size: "1024x1024",
    prompt: "把这个人物/宠物做成一套可爱的卡通贴纸，Q 版大头造型，圆润简洁线条，鲜明配色，白色背景。生成开心、大笑、生气、惊讶、害羞、疑惑、卖萌、哭泣等多种表情，每个表情清晰独立、生动有趣，适合做微信表情包。保持可辨识特征。" },
  { id: "ghibli", cat: "🔥 爆款", emoji: "🏰", label: "吉卜力/迪士尼", needRef: true, size: "1024x1536",
    prompt: "将这张照片转换为吉卜力宫崎骏动漫风格插画：柔和水彩纹理、温暖自然光、梦幻粉彩色调、大而有神的眼睛、宁静治愈氛围。保持人物可辨识，高质量。（想要迪士尼皮克斯 3D 卡通就把风格改成：圆润可爱造型、细腻 3D 质感、鲜艳色彩。）" },
  { id: "clay", cat: "🔥 爆款", emoji: "🍡", label: "粘土定格风", needRef: true, size: "1024x1024",
    prompt: "将这张照片转换成黏土定格动画风格：橡皮泥/黏土材质，柔软哑光表面，可见手工捏制的指纹细节，圆润可爱造型，马卡龙柔和配色，工作室柔光，3D 渲染，类似《小羊肖恩》的手工质感。保持人物特征可辨识。" },
  { id: "pet-human", cat: "🔥 爆款", emoji: "🐾", label: "宠物拟人化", needRef: true, size: "1024x1536",
    prompt: "将这只宠物拟人化：直立站立，穿着时尚的人类服装（西装/卫衣），拟人化的表情、姿态和个性，保留宠物原有的毛色、花纹和面部特征使其可辨识。电影感光照，皮克斯风格 3D 渲染，高细节，趣味生动。" },
  { id: "pixel", cat: "🔥 爆款", emoji: "👾", label: "像素风", needRef: true, size: "1024x1024",
    prompt: "将这张照片/主体转换为复古像素风格：16-bit 复古游戏像素画，清晰的方块像素，有限的复古配色，轻微 dithering 抖动质感，怀旧街机游戏画面感。保持主体可辨识。" },
  { id: "comic4", cat: "🔥 爆款", emoji: "💬", label: "四格漫画", needRef: true, size: "1024x1024",
    prompt: "把这个人物/宠物做成一则搞笑四格漫画（2×2 分格），Q 版卡通形象保持可辨识特征，讲一个「日常小情景（如加班/减肥/追剧/带娃）」的有趣段子。每格配简短中文对白气泡，画风轻松可爱、线条简洁、配色明快，中文对白清晰无错字，适合发朋友圈/做表情包。",
    hint: "把「」里换成你想吐槽的日常场景" },

  // ——— 🛒 商用（电商/品牌，商家高频刚需） ———
  { id: "product-white", cat: "🛒 商用", emoji: "🛒", label: "电商白底主图", needRef: true, size: "1024x1024",
    prompt: "以上传的产品图为准，严格保持产品外观、颜色、包装和品牌标识完全一致。将产品放在纯白背景（RGB 255,255,255）正中央，正面偏 45 度角展示，柔和均匀的影棚灯光，产品底部带一层自然的接触阴影，边缘清晰锐利。画面干净，不要任何多余道具、文字、手或杂物，符合电商平台白底主图规范，8K 超清商业摄影质感。" },
  { id: "product-scene", cat: "🛒 商用", emoji: "🌿", label: "场景种草图", needRef: true, size: "1024x1536",
    prompt: "以上传的产品为视觉中心，将它自然融入真实使用场景（如北欧风客厅茶几/ins 风梳妆台/户外野餐垫）。柔和自然的窗边光，背景轻微虚化突出产品，点缀绿植/书本/咖啡杯等生活化元素，营造温馨高级的生活感，杂志社论级摄影质感，8K。产品本体保持不变。" },
  { id: "product-3view", cat: "🛒 商用", emoji: "📐", label: "产品三视图", needRef: true, size: "1536x1024",
    prompt: "以上传的产品为准，保持外观、颜色、材质完全一致。在纯白背景上，将同一产品并排展示三个角度：正面、45 度侧面、背面，三者大小一致、水平排列、光影统一，工业级产品摄影，柔和影棚布光，清晰无杂物，适合详情页，8K 超清。" },
  { id: "aplus", cat: "🛒 商用", emoji: "📊", label: "详情页卖点图", size: "1024x1536",
    prompt: "设计一张「产品名」的电商详情页卖点图，产品在高级质感背景中居中展示，四周用简洁图标和短句标注 3-4 个核心卖点（卖点1/卖点2/卖点3），图文排版整洁有层次，留白得当，欧美品牌科技感风格，配色高级，中文文案清晰、对齐、无错字。" },
  { id: "logo", cat: "🛒 商用", emoji: "✒️", label: "Logo 设计", size: "1024x1024",
    prompt: "为「品牌名/行业」设计一个简约现代的品牌 Logo。风格：极简几何/字母图形/吉祥物，主色「颜色」。造型简洁、辨识度高、可缩放，纯白背景居中，矢量扁平风，边缘干净。不要复杂渐变、不要杂乱细节。" },
  { id: "poster", cat: "🛒 商用", emoji: "🎨", label: "中文海报", size: "1024x1536",
    prompt: "设计一张商业促销海报：主标题「限时特惠」、副标题「补充你的卖点」，中文字体清晰美观、排版专业，配色高级，留出文案位置，电商风格，高清、无错字。",
    hint: "gpt-image-2 中文出字强，是它的杀手锏；把「」里换成你的文案" },
  { id: "menu", cat: "🛒 商用", emoji: "📋", label: "菜单/价目表", size: "1024x1536",
    prompt: "设计一张「咖啡店/奶茶店/理发店」价目表菜单，竖版 A4。顶部店名+Logo，下方分类列出菜品/服务名称与价格，排版清晰对齐，留白舒适，风格 ins 简约/复古手绘，配少量小图标点缀，中文清晰对齐无错字，可直接打印张贴。" },
  { id: "wechat-cover", cat: "🛒 商用", emoji: "📰", label: "公众号封面", size: "1536x1024",
    prompt: "设计一张公众号文章头图，横版宽幅。主题「文章主题」，主视觉「画面描述」，左侧留出标题空间，大标题文字「标题」醒目清晰，风格简约/科技/国潮，视觉冲击力强，构图干净，中文标题无错字。" },
  { id: "food", cat: "🛒 商用", emoji: "🍜", label: "美食诱人图", needRef: true, size: "1024x1024",
    prompt: "以上传的菜品照片为准，严格保持菜品本身的种类、食材、摆盘不变。把它修成诱人的商业美食大片：补充暖色调打光让食材有光泽、汤汁/热气更鲜活，背景轻微虚化并去除杂乱，色彩鲜亮勾起食欲，俯拍或 45 度角，专业美食杂志摄影质感，8K 超清，适合外卖平台主图/菜单/朋友圈。" },
  { id: "xhs-cover", cat: "🛒 商用", emoji: "📌", label: "小红书封面", size: "1536x2048",
    prompt: "设计一张小红书爆款封面图，竖版 3:4。主标题「你的标题（如：亲测有效！3 招搞定 XX）」大而醒目、放在视觉中心，配色高饱和吸睛（奶油/马卡龙/撞色），背景是「主题相关画面」，排版有小红书感（大字报+emoji 点缀+重点划线），中文清晰无错字，一眼抓人。",
    hint: "gpt-image-2 中文出字强；把「」里换成你的标题" },
  { id: "thumbnail", cat: "🛒 商用", emoji: "▶️", label: "视频封面/缩略图", size: "1536x1024",
    prompt: "设计一张高点击率视频封面缩略图，横版 16:9。左侧放醒目大标题「标题文案」（超粗描边字、亮黄/亮红高对比），右侧留出人物/主体位置，背景冲击力强、色彩饱和，带夸张表情或箭头/圆圈等引导元素，油管/视频号/B站风格，中文标题清晰无错字，一眼看懂主题。" },

  // ——— 🧑 人像（刚需/情感） ———
  { id: "id-photo", cat: "🧑 人像", emoji: "🪪", label: "证件照换底色", needRef: true, size: "1024x1536",
    prompt: "以上传人像为准，截取头部和肩部，制作成标准一寸/二寸证件照。要求：1、蓝色/白色/红色纯色背景；2、换成深色职业正装；3、正脸端正、表情自然微笑；4、光线均匀、五官清晰、发型整洁。保持本人五官特征不变，符合证件照规范。" },
  { id: "biz-portrait", cat: "🧑 人像", emoji: "💼", label: "职业形象照", needRef: true, size: "1024x1536",
    prompt: "以上传照片为参考，严格保持本人五官、肤色、发型一致。生成专业商务形象照：穿深色西装/职业套装，站在简洁灰色背景/现代办公室虚化背景前，自信专业的表情，柔和均匀的影棚人像光，杂志级商业人像质感，半身构图，8K 超清。" },
  { id: "wedding", cat: "🧑 人像", emoji: "💍", label: "AI 婚纱照", needRef: true, size: "1024x1536",
    prompt: "以上传的两人照片为参考，保持两人五官特征一致。生成唯美婚纱照：新娘穿白色婚纱、新郎穿黑色西装，在海边夕阳/欧式教堂/花海场景中甜蜜相依，柔和梦幻的自然光，浅景深虚化背景，电影级唯美人像摄影，高级质感。" },
  { id: "baby", cat: "🧑 人像", emoji: "👶", label: "宝宝百天照", needRef: true, size: "1024x1536",
    prompt: "以上传的宝宝照片为参考，保持宝宝五官可爱特征。生成温馨百天纪念照：宝宝躺在柔软毛毯上/被鲜花环绕，穿可爱造型服装，柔和明亮的自然光，粉嫩温暖色调，浅景深，高级儿童摄影质感，画面干净温馨，不要过度磨皮。" },
  { id: "photo-restore", cat: "🧑 人像", emoji: "🖼️", label: "老照片修复上色", needRef: true, size: "auto",
    prompt: "修复并为这张老照片上色。去除划痕、污渍、褪色和模糊，还原清晰的面部细节和纹理，自然真实地为黑白照片上色，肤色、衣服、背景颜色符合年代感，保持人物原有五官和表情不变，最终呈现高清、自然、有温度的彩色照片。" },
  { id: "restyle", cat: "🧑 人像", emoji: "💇", label: "换发型/换装", needRef: true, size: "auto",
    prompt: "以上传人像为准，严格保持本人五官、脸型、肤色不变。将发型换成短发波波头/长卷发（发色可指定），或将服装换成「你想要的」。新发型/服装贴合自然、光影一致、边缘真实，其余部分保持原样，逼真无违和。" },
  { id: "hanfu", cat: "🧑 人像", emoji: "🏮", label: "古风汉服写真", needRef: true, size: "1024x1536",
    prompt: "以上传人像为准，严格保持本人五官、脸型不变。生成唯美古风汉服写真：穿精致汉服/古装，置身古典园林/竹林/宫廷/花海场景，发型妆容符合古风，柔和唯美的自然光，浅景深虚化背景，国风摄影大片质感，高级典雅。" },

  // ——— 🛠 实用编辑 ———
  { id: "outpaint", cat: "🛠 实用", emoji: "↔️", label: "扩图/改比例", needRef: true, size: "1536x1024",
    prompt: "将这张图向四周自然扩展画面，补全被裁掉的背景和环境，保持原有主体、比例、光影、色调和风格完全一致，扩展部分与原图无缝衔接、过渡自然，看不出拼接痕迹，高分辨率。" },
  { id: "cleanup", cat: "🛠 实用", emoji: "🧹", label: "去水印/去杂物", needRef: true, size: "auto",
    prompt: "去除这张图片中的水印/文字/路人/杂物/多余物体，用符合周围环境的内容自然填补空缺，保持背景纹理、光影和透视连贯，看不出修改痕迹，其余部分保持不变，高清无损。" },
  { id: "sketch2img", cat: "🛠 实用", emoji: "📝", label: "草图转成品", needRef: true, size: "1024x1024",
    prompt: "以这张手绘草图/线稿为结构基础，渲染成精致成品图。保持草图的构图、比例和主要形态，补全真实材质/光影/色彩/细节，风格写实产品渲染/精致插画，高质量、专业成品质感。" },
  { id: "infographic", cat: "🛠 实用", emoji: "📈", label: "知识信息图", size: "1024x1536",
    prompt: "把「主题（如：高效时间管理的 5 个方法）」做成一张清晰的中文信息图/知识图解。竖版，顶部大标题，下面用图标+短句分点罗列要点，配色专业协调、层次分明、留白得当，扁平插画风，重点突出，中文清晰对齐无错字，适合发朋友圈/公众号/做 PPT。",
    hint: "把知识点/流程/对比做成一张图，选它" },
];

/** 出图尺寸 —— 只是「快捷选项」，不是限制：选择器自带手填，能填任意尺寸直透上游。
 *  ⚠️ 但 OpenAI 系作图模型（gpt-image-2，默认/推荐）官方只保证 1024x1024 / 1536x1024 / 1024x1536 / auto
 *  这几档（早先写的 1792 是 DALL·E-3 的档，gpt-image-2 会报「尺寸不支持」）。**1536x2048（3:4）已由生产实测
 *  billing 日志证实 gpt-image-2 可正常出图**，故列为预设。seedream/万相等别的模型尺寸规则不同，拿不准就用
 *  「自动」或按它的文档手填——选了不支持的尺寸，上游会把可用档位回在报错里（客户端已把这类报错翻成人话，
 *  见 providers.rs::friendlier_image_error）。
 *  ⚠️ 客户/AI 常把「3:4 比例」直接当字符串填进尺寸框（"3:4"）——这不是合法的 WxH 像素值，上游会报
 *  `size must be one of 'WIDTHxHEIGHT', '1k', '2k', or '4k'`（2026-07-03 生产日志实锤，channel #7）。
 *  给一个真正的「3:4 竖图」预设正是为了让客户不用手填、不会踩这个坑。 */
export const IMAGE_SIZES: { id: string; label: string; desc?: string; recommend?: boolean }[] = [
  { id: "1024x1024", label: "1:1 方图", desc: "正方形，头像 / 图标 / 通用，所有模型都吃，最稳。", recommend: true },
  { id: "1024x1536", label: "2:3 竖图", desc: "竖版海报 / 手机壁纸 / 朋友圈长图（要印刷竖版选它，再缩放到 600×900）。" },
  { id: "1536x2048", label: "3:4 竖图", desc: "短视频 / 抖音 / 视频号封面图常用比例，人像竖图首选。" },
  { id: "1536x1024", label: "3:2 横图", desc: "横版 banner / 电脑壁纸 / 封面。" },
  { id: "auto", label: "自动（gpt-image-2）", desc: "让模型自己挑支持的尺寸，最不易报「尺寸不支持」；个别国产模型可能不认，那就选上面的固定档。" },
];

/** 视频模型候选 —— 虾盘云 `/v1/video/generations` 异步出片。
 *  ★ 2026-07-06 全量切换火山引擎 Ark Seedance（国内直连、按条计费、失败自动退费）：
 *    veo(谷歌)/万相(阿里)/即梦(ER) 已下线——旧模型名服务端自动映射到 Seedance 对应档，老版本客户端零影响。
 *  三档都支持中文提示词、文生视频、图生视频（首帧图）。
 *  ⚠️ `price5s` 是 480p / 5 秒的预估参考价，实扣由服务器决定。 */
export type VideoModel = {
  id: string;
  label: string;
  desc?: string;
  recommend?: boolean;
  i2v?: boolean;
  /** 预估参考价，实扣由服务器决定（单位：人民币 / 5 秒 / 480p）。 */
  price5s?: number;
};

export const VIDEO_MODELS: VideoModel[] = [
  {
    id: "doubao-seedance-2-0-mini-260615",
    label: "Seedance Mini · 火山（快·省·推荐）",
    desc: "字节豆包视频模型，国内直连稳定，中文理解好，日常短视频 / 表情包首选。支持「首帧图→视频」(图生视频)。",
    recommend: true,
    i2v: true,
    price5s: 2.9,
  },
  {
    id: "doubao-seedance-2-0-fast-260128",
    label: "Seedance Fast · 火山（更清晰）",
    desc: "画质与动作细节更好，质量 / 价格平衡档。支持图生视频。",
    i2v: true,
    price5s: 4.9,
  },
  {
    id: "doubao-seedance-2-0-260128",
    label: "Seedance Pro · 火山（最高质量）",
    desc: "最高清最精细，适合要发布 / 商用的成片，最费额度。支持图生视频。",
    i2v: true,
    price5s: 6.9,
  },
];
