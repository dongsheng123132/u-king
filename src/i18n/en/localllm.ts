/** 英文覆盖字典 · 本地大模型（Local LLM，四引擎页 + 右侧模型商店）。 */
export const localllm: Record<string, string> = {
  "离线 · 免费 · 数据不出本机": "Offline · free · nothing leaves this PC",
  "没探到独立显卡": "No discrete GPU detected",
  "显存": "VRAM",
  "没有能加速推理的显卡，速度会明显慢":
    "No GPU that can accelerate inference — expect it to be noticeably slow",
  "这台机器合适的档位": "What this machine can run well",
  "这些引擎给的都是 OpenAI 兼容端点。Claude Code 和新版 Codex 认的是另外两种协议，接不了本地模型；要用本地模型请配进 ClawX 或 Hermes。":
    "These engines only expose an OpenAI-compatible endpoint. Claude Code and current Codex speak two other protocols, so they cannot use a local model — point ClawX or Hermes at it instead.",
  "正在跑": "Running",
  "可以用": "Ready",
  "这台跑不了": "Not supported here",
  "差一步": "One step short",
  "还没装": "Not installed",
  "{name} 已安装": "{name} installed",
  "已停止": "Stopped",
  "已启动，端点已就绪": "Started — endpoint is up",
  "模型目录": "Model folders",
  "已添加模型目录": "Model folder added",
  "添加目录": "Add folder",
  "已导入到 Ollama": "Imported into Ollama",
  "导入 GGUF 到 Ollama": "Import a GGUF into Ollama",

  // —— 运行时 + 参数 ——
  "当前模型": "Current model",
  "从右边选一个（本地没有就去「商店」下）": "Pick one on the right — or download one from the Store",
  "运行时": "Runtime",
  "（这台跑不了）": " (not supported here)",
  "（没装）": " (not installed)",
  "算力": "Compute",
  "自动": "Auto",
  "全部交给显卡": "All on GPU",
  "只用 CPU": "CPU only",
  "上下文": "Context",
  "端口": "Port",
  "线程": "Threads",
  "0 = 引擎自己按核心数定": "0 lets the engine pick from your core count",
  "上下文调大 = 一次能读更长的东西，但还没开口就先吃掉更多内存；显存不够时「全部交给显卡」会让引擎直接退出，拿不准就留「自动」。":
    "A bigger context reads longer input but eats memory before the first token; forcing everything onto a GPU without the VRAM makes the engine exit on load. When in doubt leave both on Auto.",
  "安装引擎": "Install engine",
  "已加进「AI 设置」当驱动": "added to AI Settings as a driver",
  "标准输出": "Standard output",
  "（还没有日志。模型第一次加载要几十秒到几分钟，进度会在这里滚。）":
    "(No logs yet. The first load takes anywhere from seconds to minutes — progress scrolls here.)",

  // —— 右侧：本地 / 商店 ——
  "本地": "Local",
  "商店": "Store",
  "刷新货架": "Refresh the shelf",
  "下载位置": "Download to",
  "改": "Change",
  "这台机器上还没有模型。去「商店」下一个，或把已有的 .gguf 放进上面那个文件夹。":
    "No models on this machine yet. Grab one from the Store, or drop a .gguf into the folder above.",
  "正在取货架…": "Fetching the shelf…",
  "货架是空的": "The shelf is empty",
  "正在问模型站有哪些量化…": "Asking the model host which quantisations exist…",
  "正在准备…": "Getting ready…",
  "{label} 已下载完成": "{label} downloaded",
  "下载位置已改到 {dir}": "Downloads now go to {dir}",
  "这台跑得动": "This machine can run it",
  "勉强，会很慢": "Tight — it will be slow",
  "内存不够（要 {n} GB）": "Not enough RAM (needs {n} GB)",
  "{n} 个分片": "{n} parts",
  "已在本地": "Already local",
  "量化档越小越省内存、答得越糙。不知道选哪个就挑 Q4 那一档 —— 它是公认的平衡点。":
    "Smaller quantisations use less memory and answer more crudely. When unsure take a Q4 — it is the accepted sweet spot.",
  "模型来自魔搭（国内直连），下载走断点续传，断了再点一次接着下。":
    "Models come from ModelScope. Downloads resume, so a dropped connection just means clicking again.",
};
