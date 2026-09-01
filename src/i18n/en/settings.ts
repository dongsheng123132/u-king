/** 英文覆盖字典 · AI 设置 / 驱动 UI（Manager + components/*）。key = 中文原文，value = English。 */
export const settings: Record<string, string> = {
  // AI 设置的分区（2026-08-21：原来四段竖排，低频块挤着高频块；08-24 加「免费额度」第五个）
  "工具分配": "Tools",
  "哪个 AI 用哪家、用什么模型": "Which provider and model each AI uses",
  "供应商库": "Providers",
  "增删改各家 API，所有 AI 共用一份": "Add / edit / remove APIs — one shared list for every AI",

  // ---- 免费额度教程页（内容本身热下发，这里只翻界面骨架）----
  "免费额度": "Free tiers",
  "哪几家现在能白嫖，Key 去哪领": "Who's giving away free quota right now, and where to get the key",
  "免费额度怎么领": "How to get free quota",
  "下面几家现在有免费档。Key 得你自己去它们官网领，领完回来「一键导入」，我们只帮你把地址和模型名填好。":
    "These providers currently offer a free tier. You get the key yourself from their site — come back and hit \"Import\" and we'll prefill the endpoint and model name for you.",
  "核实于": "Checked",
  // 「已更新」本文件下面（约 281 行）已有一条，别再登记一遍 —— 同键两处 tsc 直接报 TS1117
  "⚠️ 免费是各家自己的活动，随时可能收紧或下线，我们说了不算。重要的活还是走虾盘云。":
    "⚠️ These free tiers are each provider's own promotion — they can tighten or pull them anytime, and that's not up to us. For work that matters, stick with Xiapan Cloud.",
  "去领 Key": "Get a key",
  "一键导入": "Import",
  "没有你想要的那家？「供应商库」里有 20 家模板，或者自己手填地址也行。":
    "Don't see the one you want? There are 20 templates under Providers, or just type the endpoint in yourself.",

  // ---- 工具体检（ToolCheckup）----
  "工具体检": "Tool checkup",
  "{n} 个工具装好了还没接 AI": "{n} installed tools are not connected to AI yet",
  "{n} 个 AI 助手全部就绪 ✅": "All {n} AI assistants are ready ✅",
  "已自行配置": "Self-managed",
  "配好了，去终端试试": "Configured — try it in Terminal",
  "暂不支持自动配置": "Auto-configuration is not supported yet",
  "配置好像没写进去，点「重试」再试一次": "The configuration doesn't look applied — hit Retry to try again",
  "重试": "Retry",
  "检测到 {name} 装好还没接 AI，点一下用内置额度让它开口～":
    "{name} is installed but not connected to AI. Use the built-in balance to get it talking in one click.",
  "用 U-King 内置虾盘云额度给它配好，并确认配置写上、虾盘云连通":
    "Configure it with U-King's built-in Xiapan Cloud balance, verify the config was written, and confirm the cloud is reachable.",
  "Key 校验没过": "The key could not be verified",
  "网络不通": "Network connection failed",
  "暂时没配上，可以稍后再试": "Could not configure it just now — please try again later",

  "用量账单": "Usage",
  "钱花在哪了": "Where the money went",
  "高级": "Advanced",
  "桌面 App / Codex 专区": "Desktop apps / Codex zone",
  "统一供应商库": "Shared provider library",
  "供应商只登记一次，Claude Code / Codex / ClawX / Hermes 都从这里引用。改一处，用到它的 AI 全都跟着变。":
    "Register a provider once — Claude Code, Codex, ClawX and Hermes all reference it from here. Edit it in one place and every AI using it follows.",
  "一处登记，所有 AI 共用，Key 只填一次。改一处，用到它的 AI 全都跟着变。":
    "Register once, share across every AI, and enter the key only once. Edit it in one place and every AI using it follows.",
  "地址": "Endpoint",
  "正在被 {tools} 使用": "In use by {tools}",
  "已在 {n}/{total} 个工具启用:{tools}": "Enabled in {n}/{total} tools: {tools}",

  // 设备钱包挂到虾盘云供应商卡片上（2026-08-22）——钱包是这家供应商的一部分，不是全局功能
  "设备钱包": "Device wallet",
  "余额 · 充值 · 换一把 Key": "Balance · top up · rotate key",

  // 添加供应商弹窗：存前试连 + 存前拉模型（2026-08-22）
  "填好地址和 Key 后点「拉取」，从这家真实有的模型里选 —— 不用去官网抄":
    "Fill in the endpoint and key, then click Fetch to pick from the models this provider actually has — no need to copy from their docs",
  "从接口拉取真实模型清单；部分供应商允许匿名读取，Key 请用「测试连通」验证":
    "Fetch the real model list; some providers allow anonymous access, so verify the key with Test connection",
  "拉取": "Fetch",
  "U-King 内置 · 一键加回当前 AI": "U-King built-in · restore to this AI",
  "预设供应商 · 预填接口，Key 由你自己填写": "Provider templates · prefill endpoint; enter your own key",
  "✓ 拉到 {n} 个模型 —— 接口地址可达；Key 请点「测试连通」确认。点输入框从清单里选":
    "✓ Got {n} models — the URL is reachable. Use Test connection to verify the key. Click the input to pick from the list",
  "「{reply}」· {ms}ms · 可以保存了": "“{reply}” · {ms}ms · safe to save",
  "用当前填的地址 / Key / 模型真发一条消息 —— 通了再保存，错了当场看到原因":
    "Send a real message with the URL / key / model as filled — save once it works; see the error right here if it doesn't",

  // 「AI 作图」方格（2026-08-22 作图解绑虾盘云）：虾盘云从「唯一」降级成「默认」
  "作图 / 图生图用哪家": "Which provider draws and edits images",
  "内置（虾盘云钱包计费）": "Built-in (billed to the Xiapan Cloud wallet)",
  "走 {name}": "Via {name}",
  "供应商": "Provider",
  "模型在「AI 作图」页那个下拉里选（带优缺点说明，随时能换一家上游）":
    "Pick the model from the dropdown on the AI Image page — it lists each model's trade-offs so you can switch upstreams anytime",
  "模型（必填）": "Model (required)",
  "这家的作图模型 id，如 flux-pro / dall-e-3": "That provider's image model id, e.g. flux-pro / dall-e-3",
  "应用": "Apply",
  "默认：走内置虾盘云端点，用这台机器的钱包 Key 计费":
    "Default: the built-in Xiapan Cloud endpoint, billed to this machine's wallet key",
  "用这家自己的 Key 计费（在「供应商库」里填），不再从虾盘云钱包扣钱":
    "Billed with that provider's own key (set it under Providers) — nothing is drawn from the Xiapan Cloud wallet",
  "AI 作图走虾盘云（内置 Key 计费），模型在「AI 作图」页选":
    "Images go through Xiapan Cloud (billed with the built-in key); pick the model on the AI Image page",
  "AI 作图已改走 {name} —— 用它自己的 Key 计费":
    "Images now go through {name}, billed with its own key",

  // 通用（跨组件复用）
  取消: "Cancel",
  保存: "Save",
  编辑: "Edit",
  删除: "Delete",
  内置: "Built-in",
  自定义: "Custom",
  推荐: "Recommended",
  名称: "Name",
  模型: "Model",
  使用中: "In use",
  已装: "Installed",
  未装: "Not installed",
  充值: "Top up",
  下载: "Download",
  关闭: "Close",

  // ── Manager.tsx ──
  // token 数字单位（数值已除以 1 万；×10k 保持数值不变，仅换单位说明）
  万: "×10k",
  "{model}（Codex 推荐）": "{model} (Codex recommended)",
  "{model}（预设默认）": "{model} (preset default)",

  // 余额 / 用量趋势
  虾盘云余额: "Xiapan Cloud balance",
  待充值: "Top up needed",
  "余额偏低，Codex 大模型可能不够一次请求":
    "Balance is low; may not cover one large-model Codex request",
  "AI 按量扣费，不用不扣": "AI is billed by usage — no charge when idle",
  "余额不足，请充值后使用 AI": "Insufficient balance — top up to use AI",
  补充余额: "Add balance",
  充值开通: "Top up to activate",
  刷新: "Refresh",
  已刷新余额: "Balance refreshed",
  "每日消耗（最近 14 天）": "Daily usage (last 14 days)",
  今日: "Today",
  "近 7 天": "Last 7 days",
  "暂无数据 —— 多用几天、多刷新几次余额就有曲线了":
    "No data yet — use it a few days and refresh the balance to see the trend",
  "最近 {n} 天，钱花在哪了": "Where your money went (last {n} days)",
  "{n} 次": "{n}×",

  // 切换 / 测试 / 拉模型 的 toast
  "请先填入 Key，再拉取模型清单": "Enter a key first, then fetch the model list",
  "已拉取 {n} 个可用模型 —— 下拉里选，或直接手填":
    "Fetched {n} available models — pick from the dropdown or type manually",
  "拉取失败：{e} —— 可直接手填模型 id": "Fetch failed: {e} — you can type the model id manually",
  "请先填入 {name} 的 API Key": "Enter the API Key for {name} first",
  "（需重启 ClawX 生效）": " (restart ClawX to take effect)",
  "（已自动重启 ClawX 生效）": " (ClawX auto-restarted, now live)",
  "（已走 DeepSeek 省钱路由）": " (on the DeepSeek money-saving route)",
  "已把 {tool} 切到 {name}{model}{hint}": "Switched {tool} to {name}{model}{hint}",
  "切换失败：{e}": "Switch failed: {e}",
  "请先填入 Key 再测试": "Enter a key before testing",
  "{name} 连通 ✓ {ms}ms": "{name} connected ✓ {ms}ms",
  "{name} 测试失败": "{name} test failed",
  "测试异常：{e}": "Test error: {e}",
  "让 AI 帮我修": "Let AI fix it",
  "已把故障交给 AI，正在打开工作台": "Handed to AI, opening workspace",
  "已保存「{name}」": "Saved “{name}”",
  "保存失败：{e}": "Save failed: {e}",
  "已删除「{name}」": "Deleted “{name}”",
  "删除失败：{e}": "Delete failed: {e}",

  // 供应商列表是 per-tool 的：在一个 AI 里移除，其它 AI 照旧留着
  "把「{name}」从 {tool} 的列表里移除？": "Remove “{name}” from {tool}'s list?",
  "只影响 {tool} —— {others} 的列表照旧留着它，需要时可在下方或「添加供应商」里加回来。":
    "This only affects {tool} — {others} keep it in their own lists. Add it back below or via “Add provider” whenever you want.",
  "⚠️ {tool} 正在用它。移除只影响这个列表，**不会改动它已配好的配置** —— 要换请另选一个供应商启用，或选「官方直连（还原）」。":
    "⚠️ {tool} is currently using it. Removing only affects this list — **it does not change the config already written** — to switch, enable another provider or pick “Official direct (restore)”.",
  "这只影响列表显示，不会改动你已经配好的任何 AI 工具。":
    "This only affects what the list shows; none of your configured AI tools are touched.",
  "已从 {tool} 的列表移除「{name}」（其它 AI 保留）":
    "Removed “{name}” from {tool}'s list (other AIs keep it)",
  "已把「{name}」加回 {tool} 的列表": "Added “{name}” back to {tool}'s list",
  "从供应商库添加": "Add from provider library",
  "引用供应商": "Reference provider",
  "勾选未在当前 AI 列表的供应商即可引用，Key 无需再次填写。":
    "Select a provider not already in this AI's list to reference it; no need to enter the key again.",
  "供应商库还没有供应商。先新建一家吧。": "There are no providers in the library yet. Create one first.",
  "已在当前 AI 列表": "Already in this AI's list",
  "引用到 {tool}": "Reference in {tool}",
  "找不到要用的供应商？": "Can't find the provider you need?",
  "新建供应商…": "Create provider…",
  "供应商库里已有「{name}」使用相同地址。\n\n确认：更新它（保留原 Key，除非你改填）。\n取消：继续选择「仍新建（用于多账号）」或「取消」。":
    "The provider library already has “{name}” at the same endpoint.\n\nConfirm: update it (keep its existing key unless you enter a new one).\nCancel: continue to choose “Create another (for multiple accounts)” or “Cancel”.",
  "仍新建「{name}」？仅在需要为多账号保留独立实例时使用。\n\n确认：仍新建（用于多账号）。\n取消：取消保存。":
    "Create another “{name}”? Use this only when separate instances are needed for multiple accounts.\n\nConfirm: create another (for multiple accounts).\nCancel: cancel saving.",
  "「{name}」是内置驱动，无需新建 —— 直接引用它？":
    "“{name}” is a built-in provider — no need to create another. Reference it directly?",
  "确认：把内置的 {name} 加回当前 AI 的列表（Key 在列表行里填）。取消：返回修改。":
    "Confirm: add the built-in {name} back to this AI's list (enter the key on its list row). Cancel: return to editing.",
  "已从 {tool} 移除：": "Removed from {tool}:",
  "添加供应商 —— 内置一键加回，其它自己填 Key":
    "Add a provider — one click to restore a built-in, or fill in your own key",
  "添加：预填地址/模型，进弹窗只需补 Key":
    "Add: endpoint/model prefilled, just fill in your key in the dialog",
  "从 {tool} 的列表移除（其它 AI 保留）": "Remove from {tool}'s list (other AIs keep it)",
  "{tool} 的列表是空的 —— 你把它的供应商都移除了。":
    "{tool}'s list is empty — you removed every provider from it.",
  "点上面「+ 从供应商库添加」引用已有的，或在下方把移除掉的加回来。别的 AI 不受影响。":
    "Use “+ Add from provider library” above to reference an existing provider, or add a removed one back below. Other AIs are unaffected.",
  "一键加进 {tool} 的列表": "Add to {tool}'s list in one click",
  "从这个 AI 的列表移除（其它 AI 保留）": "Remove from this AI's list (other AIs keep it)",
  "已从这个 AI 的列表移除 {name}（其它 AI 保留）":
    "Removed {name} from this AI's list (other AIs keep it)",

  // 彻底删除 = 全部 AI + 定义 + Key，只放在编辑弹窗里
  彻底删除: "Delete for good",
  "从全部 AI 的列表里删掉，并销毁它的地址和已保存的 Key":
    "Remove it from every AI's list and destroy its endpoint and saved key",
  "彻底删除「{name}」？": "Delete “{name}” for good?",
  "会从**全部 4 个 AI** 的列表里拿掉，连同它的地址和已保存的 API Key 一起删除，之后只能重新填一次。":
    "It will be dropped from **all 4 AIs'** lists, and its endpoint and saved API key deleted — you would have to enter it again from scratch.",
  "⚠️ {tools} 正在用它。删除只动这份列表，**不会改动它们已配好的配置**，但你在这里就换不回它了。":
    "⚠️ {tools} currently use it. Deleting only touches this list — **their existing configs are left alone** — but you will no longer be able to switch back to it here.",
  "不会改动你已经配好的任何 AI 工具。": "None of your configured AI tools are touched.",
  "已彻底删除「{name}」": "Deleted “{name}” for good",
  "彻底删除（全部 AI + 已保存的 Key）": "Delete for good (all AIs + saved key)",
  "已彻底删除 {name}（全部 AI + 已保存的 Key）": "Deleted {name} for good (all AIs + saved key)",

  // 工具 Tab / 添加供应商 / 教程入口
  已接管: "Managed",
  未接管: "Not set",
  配置中: "Editing",
  "选择要配置的 AI": "Choose an AI to configure",
  // 「左装右选 → 合并启动」（2026-08-20）
  "未安装": "Not installed",
  "装好 {name} 并启动": "Install {name} and launch",
  "还没装 —— 会先走装机流程，装完再按上面选好的驱动启动":
    "Not installed yet — this runs the installer first, then launches with the driver you picked above.",
  "ClawX 是独立桌面应用，会单独开一个窗口": "ClawX is a standalone desktop app; it opens in its own window.",
  "按上面选好的驱动，在 U-CLI 里开一个配好的会话（想拉出去有「拉出」按钮）":
    "Opens a ready-configured session in U-CLI using the driver picked above (use “pop out” if you want a separate window).",
  "每个 AI 各自独立 —— 驱动、模型、连这份供应商列表都是分开的（在这里删掉，别的 AI 照旧留着）":
    "Every AI is independent — driver, model, and even this provider list are separate (remove one here and the other AIs keep theirs)",
  "未配置 · 点此设置": "Not configured · click to set up",
  官方直连: "Official direct",
  添加供应商: "Add provider",
  "ClawX / Hermes 桌面版改用「复制 Key 到设置」的图文教程，最稳":
    "For ClawX / Hermes desktop, use the “copy key into settings” illustrated guide — most reliable",
  "ClawX / Hermes 桌面版？看教程配 →": "ClawX / Hermes desktop? See the setup guide →",
  "Hermes 桌面版改用「复制 Key 到设置」的图文教程，最稳":
    "For Hermes desktop, use the “copy key into settings” illustrated guide — most reliable",
  "Hermes 桌面版？看教程配 →": "Hermes desktop? See the setup guide →",
  "ClawX 可在上方 Tab 一键切；Hermes 走图文教程":
    "ClawX: switch in the tabs above; Hermes: use the illustrated guide",
  "各模型获取 API Key 教程": "How to get an API Key for each model",
  "检测到 Codex 桌面版：它和 CLI 共用同一份配置，这里切一次两边都生效，不用另外配。":
    "Codex desktop app detected: it shares one config file with the CLI, so switching here applies to both — no separate setup needed.",

  // 桌面 App 状态条
  "桌面 App（图形版）": "Desktop apps (GUI)",
  "配置走「复制 Key 到设置」图文教程，最稳":
    "Configure via the “copy key into settings” guide — most reliable",
  待配置模型: "Model not set",
  "去「我的 AI」安装": "Install from “My AI”",
  "配置 →": "Configure →",

  // 用自己的 Key 板块
  "用自己的 Key？各模型官网与申请地址": "Use your own key? Official sites and where to apply",
  "想接自家账号的 API（DeepSeek / 智谱 / Kimi / 通义 / OpenAI…）？点「申请 Key」去官网创建，再回上面":
    "Want to use your own account's API (DeepSeek / Zhipu / Kimi / Tongyi / OpenAI…)? Click “Apply for key” to create one on the official site, then go back up to",
  "「+ 添加供应商」": "“+ Add provider”",
  "选对应预设填进来。": " and fill in the matching preset.",
  "申请 Key": "Apply for key",
  官网介绍: "Official site",

  // Codex 专区入口
  "Codex 专区": "Codex Zone",
  "Codex 桌面版装机 · 驱动接管 · computer use 教程":
    "Codex desktop install · provider takeover · computer use guide",
  "进入 →": "Enter →",
  // 「我的 AI」页顶部那张「AI 设置」入口卡（2026-08-25）
  "换模型 · 余额 · 免费额度 · 用自己的 Key": "Switch model · balance · free tiers · bring your own key",

  // ToolProviderList
  "还没安装 —— 先在这里选好驱动，再点下面的「装好并启动」，装完即按这套配置生效。":
    "Not installed yet — pick a driver here, then hit “Install and launch” below; it takes effect with this setup right after the install.",
  还原官方直连: "Restore official direct",
  "（未选模型）": "(no model selected)",
  测试连通: "Connectivity test",
  "获取 Key": "Get key",
  // Codex 模型提示 —— 按供应商分三条，源在 lib/models.ts::codexProtocolHint
  "Codex 只认 Responses 协议。虾盘云请用 deepseek-v4-flash-codex（默认，已验证，跟 deepseek-v4-flash 同价）—— 填裸名 deepseek-v4-flash 会报 500 convert_request_failed。想更强可选 gpt-5.3-codex，但它贵十几到几十倍（输出那头差得最狠），别当日常。":
    "Codex speaks only the Responses protocol. On Xiapan Cloud use deepseek-v4-flash-codex (the default; verified, same price as deepseek-v4-flash) — the bare name deepseek-v4-flash returns 500 convert_request_failed. gpt-5.3-codex is stronger but 12–50x more expensive (worst on output); don't make it your daily driver.",
  "Codex 只认 Responses 协议。DeepSeek 官方请用裸名 deepseek-v4-flash（已验证）—— deepseek-v4-pro 官方还没开放 Codex，选了会被直接拒绝。":
    "Codex speaks only the Responses protocol. With DeepSeek official, use the bare name deepseek-v4-flash (verified) — deepseek-v4-pro isn't open to Codex yet and will be rejected outright.",
  "GLM / Kimi 官方没有 Responses 接口，U-King 只能按老的 chat 协议配 —— 只有 0.8x 老版 Codex 认。要用新版 Codex，请换虾盘云或 DeepSeek 官方。":
    "GLM / Kimi official have no Responses endpoint, so U-King can only configure the older chat protocol — which only Codex 0.8x accepts. For a current Codex, switch to Xiapan Cloud or DeepSeek official.",
  "Codex 固定使用新版 Responses 协议；保存前请确认供应商支持 /responses。":
    "Codex always uses the newer Responses protocol; before saving, make sure the provider supports /responses.",
  "默认用内置 Key，可覆盖": "Uses built-in key by default; can override",
  还原官方登录: "Restore official login",
  应用新模型: "Apply new model",
  启用: "Enable",

  // ModelPicker
  "模型 id：下拉选，或直接手填": "Model id: pick from the list or type manually",
  "⚠️ 海外前沿旗舰，最费额度 —— 满上下文单次可能扣掉几块钱（约国产 DeepSeek 的几十倍），一天能烧掉上百元。日常强烈建议用 DeepSeek / 国产模型，确有需要再用它。":
    "⚠️ Overseas frontier flagship — the most expensive by far. A single full-context call can cost several yuan (tens of times DeepSeek), burning hundreds of yuan a day. For daily use, strongly prefer DeepSeek / domestic models; only use this when truly needed.",
  "⚠️ 海外推理模型，费额度较高。日常任务用 DeepSeek / 国产更省，确需强推理再用。":
    "⚠️ Overseas reasoning model — fairly expensive. For everyday tasks DeepSeek / domestic models are cheaper; use this only when you need heavy reasoning.",
  拉取该供应商真实可用的模型清单: "Fetch this provider's actual available model list",

  // CustomProviderModal
  编辑供应商: "Edit provider",
  "U-King 内置 · 一键添加": "Built into U-King · one-click add",
  免填表: "no form to fill",
  "内置 Key，免注册": "Key included, no signup",
  // 带空格的中文键**必须加引号**（`需自备 API Key` 里那个空格会让 TS 当场语法错误）。
  // 「中文即 key」这套写法下，只要 key 里出现空格 / 标点，就得引号包起来。
  "需自备 API Key": "Bring your own API key",
  "下面是自己填 —— 任何 OpenAI 兼容的中转 / 官方接口都能加。":
    "Below you can add your own — any OpenAI-compatible relay or official endpoint works.",
  预设供应商: "Preset providers",
  "💡 点一个自动填好接口地址，下方只需补 API Key；选「自定义」则全部手填。存好后可在列表里「🔄 拉取」选具体模型。":
    "💡 Click one to auto-fill the endpoint, then just add the API Key below. Choose “Custom” to fill everything manually. After saving, use “🔄 Fetch” in the list to pick a specific model.",
  "给这个供应商起个名字，如「我的中转」": "Give this provider a name, e.g. “My relay”",
  我的供应商: "My provider",
  "接口地址 (Base URL)": "Endpoint (Base URL)",
  "OpenAI 兼容接口，一般以 /v1 结尾": "OpenAI-compatible endpoint, usually ending in /v1",
  "默认使用的模型名（由你的接口决定）": "Default model name (determined by your endpoint)",
  "＋ 高级（小模型 / Claude 格式地址，可不填）":
    "＋ Advanced (small model / Claude-format URL, optional)",
  "省 token 的轻量模型，留空 = 同上": "Lightweight model to save tokens; leave blank = same as above",
  留空则用上面的模型: "Leave blank to use the model above",
  "给 Claude Code 用的 Anthropic 格式地址；纯 OpenAI 接口留空":
    "Anthropic-format URL for Claude Code; leave blank for OpenAI-only endpoints",
  "留空 = 仅 OpenAI 兼容": "Blank = OpenAI-compatible only",

  // ── ProviderManager.tsx ──
  请填写名称: "Please enter a name",
  "至少填一个端点（OpenAI 兼容地址 或 Anthropic 地址）":
    "Enter at least one endpoint (OpenAI-compatible URL or Anthropic URL)",
  已更新: "Updated",
  "已添加自定义 provider": "Custom provider added",
  "已删除 {name}": "Deleted {name}",
  "管理驱动 / 中转站": "Manage providers / relays",
  只读: "Read-only",
  "添加自定义 provider": "Add custom provider",
  "编辑：{name}": "Edit: {name}",
  "新建自定义 provider": "New custom provider",
  "OpenAI 兼容端点（Codex/ClawX/Hermes 用）": "OpenAI-compatible endpoint (for Codex/ClawX/Hermes)",
  "Anthropic 端点（Claude Code 用，可空）": "Anthropic endpoint (for Claude Code, optional)",
  默认模型: "Default model",
  小任务模型: "Small-task model",
  "Codex 模型（可空）": "Codex model (optional)",
  沿用默认模型: "Use default model",
  "Codex 协议": "Codex protocol",
  "chat（老版/通用）": "chat (legacy / general)",
  "responses（新版 Codex）": "responses (new Codex)",
  "保存中…": "Saving…",
  我的中转站: "My relay",

  // ── ProviderSwitch.tsx ──
  "{name} 需要先在「AI 设置」填 Key": "{name} needs a key set in “AI Settings” first",
  "，请重启 ClawX 生效": ", restart ClawX to take effect",
  "已还原官方配置{hint}": "Restored official config{hint}",
  "已切到 {name}{model}{hint}": "Switched to {name}{model}{hint}",
  "当前：": "Current: ",
  未配置: "Not configured",
  // 「Codex 默认已是…」那两条已删：中文原串 2026-08-17 grep 全仓 0 命中 —— 组件早改了、
  // 译文留着当孤儿。同一句话当时活了四份（Manager / ProviderSwitch / 这条孤儿 / backfill 三段拼接），
  // 四份里有三份说法不一致 —— 这就是收进 codexProtocolHint 的直接理由。
  换模型: "Switch model",
  添加自定义: "Add custom",
  "余额 / 填 Key": "Balance / Enter key",

  // ── ModelMenu.tsx ──
  "或手填模型 id，回车确认": "Or type a model id and press Enter",

  // ── CopyKey.tsx ──
  "已复制 API Key": "API Key copied",
  "复制失败，请手动选中复制": "Copy failed — please select and copy manually",
  已复制: "Copied",
  "复制 Key": "Copy key",

  // ── Lightbox.tsx ──
  "滚轮缩放 · 拖拽移动 · 双击复位 · Esc 关闭":
    "Scroll to zoom · drag to move · double-click to reset · Esc to close",
  缩小: "Zoom out",
  放大: "Zoom in",
  复位: "Reset",
  放大查看: "Enlarged view",

  // lib/models.ts —— 模型清单（Manager 换模型 + 工作台输入框的模型下拉共用）
  "📷 看图识图（能读图片 / 截图 / CAD 图纸）": "📷 Vision (reads images / screenshots / CAD drawings)",
  "DeepSeek V4 Flash · 最快最省（默认）": "DeepSeek V4 Flash · fastest & cheapest (default)",
  "DeepSeek V4 Pro · 满血推理": "DeepSeek V4 Pro · full reasoning",
  "Kimi K3 · 超长上下文": "Kimi K3 · very long context",
  "MiniMax M3 · 海螺": "MiniMax M3 · Hailuo",
  "GLM-5.2 · 智谱": "GLM-5.2 · Zhipu",
  "Qwen3.7 Max · 通义": "Qwen3.7 Max · Tongyi",
  "Gemini 3.5 Flash · 极速": "Gemini 3.5 Flash · very fast",
  "通义 Qwen-VL Max · 看图最准": "Tongyi Qwen-VL Max · most accurate at images",
  "通义 Qwen3-VL Plus · 新一代看图": "Tongyi Qwen3-VL Plus · newer vision model",
  "Claude Sonnet 5 · 编程之王": "Claude Sonnet 5 · best at coding",
  "Claude Opus 4.8 · 顶配旗舰": "Claude Opus 4.8 · top-tier flagship",
  "Claude Opus 4.7 · 强力推理": "Claude Opus 4.7 · strong reasoning",
  "Claude Haiku 4.5 · 轻快": "Claude Haiku 4.5 · light & quick",
  "GPT-5.4 · OpenAI 旗舰": "GPT-5.4 · OpenAI flagship",
  "Gemini 3.1 Pro": "Gemini 3.1 Pro",
  "Grok 4.5 · xAI": "Grok 4.5 · xAI",

  // 切换回验（2026-08-24）。措辞刻意区分三种状态：读到了 / 被压着 / 没读到。
  // 「没读到」绝不能翻成任何听起来像「没问题」的词 —— 空结果有两义，
  // 把「没查」说成「没事」正是这批 bug 的成因。
  "实际：{runs}": "Actually runs: {runs}",
  "实际：还没配任何驱动": "Actually runs: no provider configured yet",
  "⚠ {f} 压着它 → 实际跑 {runs}": "⚠ {f} overrides us → actually runs {runs}",
  "未回读 · 这个工具还没有回读通道": "Not verified · no read-back path for this tool yet",
  "未回读 · {f} 挡住了视线（带注释，读不了）":
    "Not verified · {f} blocks the view (has comments, cannot parse)",

  // 供应商行的延迟徽标 + 全部测速（2026-08-24）。
  // 「未测」同理不能翻成任何听起来像「没问题」的词。
  "未测": "not tested",
  "还没测过这一家": "This one has not been tested yet",
  "不通": "unreachable",
  "全部测速": "Test all",
  "测速中 {done}/{total}": "Testing {done}/{total}",
  "依次让每一家真回一句话 —— 会消耗少量额度":
    "Ask each one for a real reply, one at a time — uses a small amount of credit",
  "没有可测的供应商 —— 先填一个 Key": "Nothing to test — enter an API key first",
  "测速完成 —— 绿色 <200ms、黄色 <500ms、红色更慢或不通":
    "Done — green <200ms, amber <500ms, red is slower or unreachable",

  // ── 免费路线接入抽屉 ──
  "验证失败，尚未启用到任何 AI；不会扣虾盘余额": "Verification failed; no AI was changed and no Xiapan wallet credit was used",
  "已启用到 {tool}；真实请求验证成功，不使用虾盘钱包": "Enabled for {tool}; a real request succeeded and no Xiapan wallet is used",
  "免费算力": "Free compute",
  "国内稳定额度 + 海外/第三方免费路线": "Domestic stable credit + overseas/third-party free routes",
  "国内稳定额度": "Domestic stable credit",
  "虾盘云设备钱包：余额、充值和售后由 U-King 管；适合重要任务，不与下方第三方免费额度混算。": "Xiapan device wallet: U-King manages balance, top-ups and support. It is for important work and is separate from third-party free tiers.",
  "去配置": "Configure",
  "海外 / 第三方免费算力": "Overseas / third-party free compute",
  "下面是第三方当前公开的免费档或试用入口。U-King 不收 Key；你在官网领取后回到这里继续接入。": "These are currently public third-party free-tier or trial routes. U-King never collects keys; get one from the provider, then return here to connect it.",
  "⚠️ 我们不承诺长期免费，也不自动上线新渠道。断网时只展示最后可信清单，可能已过期，不建议直接启用。": "⚠️ We do not promise long-term free access or automatically list new routes. Offline, only the last trusted list is shown; it may be stale and is not recommended for one-click enabling.",
  "我已有 Key，继续": "I already have a key",
  "正在接入：{name}": "Connecting: {name}",
  "免费档": "Free tier",
  "第三方": "Third party",
  "已添加：Key 和供应商已保存到本机，尚未启用给任何 AI。": "Added: the key and provider are saved locally, but not enabled for any AI yet.",
  "默认：仅此第三方来源；不使用虾盘钱包，不扣费。": "Default: this third-party source only; no Xiapan wallet and no U-King charge.",
  "验证并启用中…": "Verifying and enabling…",
  "启用到 {tool}": "Enable for {tool}",
  "保存 Key 和供应商": "Save key and provider",

  // ── 一键体检 / 一键升级（DoctorCard，2026-08-31）──
  "AI 一键体检": "One-click AI checkup",
  "✅ 环境正常 · 刚检查 {time}": "✅ Environment healthy · checked at {time}",
  "点击查看体检详情": "View checkup details",
  "{ready}/{total} 个 AI 已配好": "{ready}/{total} AIs configured",
  "体检中…": "Checking…",
  "重新体检": "Check again",
  "一键升级全部 AI": "Update all AI tools",
  "升级中…": "Updating…",
  "失败": "Failed",
  "U-King 本体": "U-King app",
  "有新版 v{ver}": "New version v{ver}",
  "去升级": "Update now",
  "已是最新版": "Up to date",
  "检查不到更新（网络？）": "Unable to check for updates (network?)",
  // 「待充值」113 行已有（Top up needed），不重复 —— 同一中文映射同一英文
  "余额偏低": "Balance is low",
  "去充值": "Top up",
  "钱包状态读取失败，请重试体检": "Could not read wallet status — run the checkup again",
  "运行环境": "Environment",
  "未检测到": "Not detected",
  "便携 Node ✓": "Portable Node ✓",
  "系统代理 {p}": "System proxy {p}",
  "检测到系统代理 —— 代理节点失效时会出现「实测全绿、工具连不上」":
    "System proxy detected — a dead proxy can make checks pass while tools still fail to connect",
  "已配好": "Ready",
  "未接 AI": "Not connected",
  // 「已自行配置」28 行已有（Self-managed），这里不再重复 —— 同一中文映射同一英文
  "标黄的还没接 AI —— 用上方「一键配好」或到「免费算力」页接入":
    "Amber items are not connected yet — use “One-click setup” above or the Free compute tab",
  // 升级跳过（未安装）提示 —— 后端返回中文原句，前端按「未安装」关键词分流
  "未安装 —— 先去装机向导安装，装了才谈得上升级":
    "Not installed — install it first from the setup wizard; there is nothing to update yet",

  // ── Free Router 本地免费路由（FreerouterCard，2026-08-31）──
  "Free Router · 本地免费路由": "Free Router · local free router",
  "运行中": "Running",
  "把 OpenRouter 免费模型汇成一个本地接口，限流/下架自动换下一家 —— 需要先有 OpenRouter Key":
    "Bundles OpenRouter free models into one local endpoint and auto-fails-over when one is rate-limited or retired — requires an OpenRouter key first",
  "一键安装": "Install",
  "启动": "Start",
  "停止": "Stop",
  "Free Router 已安装": "Free Router installed",
  "Free Router 已在后台运行：": "Free Router is running in the background: ",
  "已停止": "Stopped",
  "安装失败：": "Install failed: ",
  "OpenRouter Key ✓ 已配置": "OpenRouter key ✓ configured",
  "还没有配 OpenRouter Key": "No OpenRouter key yet",
  "填 Key": "Enter key",
  "Key 已保存到本机 .env（不会上传）": "Key saved to the local .env file (never uploaded)",
  "本地接口": "Local endpoint",
  "模型名": "model name",
  "在「供应商库」添加自定义供应商填这个地址即可把任何 OpenAI 兼容工具接上来（仅本机可访问）":
    "Add a custom provider in the Provider library with this address to connect any OpenAI-compatible tool (localhost only)",
  "版本": "ver",
};
