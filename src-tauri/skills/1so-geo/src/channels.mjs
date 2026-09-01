// 一搜商答 / 1so —— 全网渠道表（%s 为查询占位符）。
// 渠道配置（唯一真相源），含搜索引擎 / AI 问答 / 社媒 / 地图 等类。
// 用途：一键生成"去 N 个平台搜你自己"的体检面板，看公司在互联网里的样子（SEO+GEO 融合）。
// favicon 复用 google s2 服务，面板观感统一。

export const CHANNELS = {
  aiSearch: {
    name: "AI 搜索", icon: "🤖",
    engines: [
      { id: "perplexity", name: "Perplexity", url: "https://www.perplexity.ai/search?q=%s" },
      { id: "metaso", name: "秘塔 Metaso", url: "https://metaso.cn/?q=%s" },
      { id: "genspark", name: "Genspark", url: "https://www.genspark.ai/search?q=%s" },
      { id: "felo", name: "Felo", url: "https://felo.ai/search?q=%s" },
      { id: "you", name: "You.com", url: "https://you.com/search?q=%s" },
      { id: "exa", name: "Exa", url: "https://exa.ai/search?q=%s" },
    ],
  },
  aiChat: {
    name: "AI 对话", icon: "💬",
    engines: [
      { id: "doubao", name: "豆包", url: "https://www.doubao.com/chat/?q=%s" },
      { id: "deepseek", name: "DeepSeek", url: "https://chat.deepseek.com/?q=%s" },
      { id: "kimi", name: "Kimi", url: "https://kimi.moonshot.cn/?q=%s" },
      { id: "yuanbao", name: "腾讯元宝", url: "https://yuanbao.tencent.com/chat?q=%s" },
      { id: "wenxin", name: "文心一言", url: "https://yiyan.baidu.com/?q=%s" },
      { id: "tongyi", name: "通义千问", url: "https://tongyi.aliyun.com/qianwen/?q=%s" },
      { id: "chatglm", name: "智谱清言", url: "https://chatglm.cn/?q=%s" },
      { id: "minimax", name: "海螺 MiniMax", url: "https://hailuoai.com/?q=%s" },
      { id: "mimo", name: "小米 MiMo", url: "https://aistudio.xiaomimimo.com/?q=%s" },
      { id: "xinghuo", name: "讯飞星火", url: "https://xinghuo.xfyun.cn/desk?q=%s" },
      { id: "tiangong", name: "昆仑天工", url: "https://www.tiangong.cn/?q=%s" },
      { id: "yuewen", name: "阶跃跃问", url: "https://yuewen.cn/?q=%s" },
      { id: "chatgpt", name: "ChatGPT", url: "https://chatgpt.com/?q=%s" },
      { id: "claude", name: "Claude", url: "https://claude.ai/new?q=%s" },
      { id: "gemini", name: "Gemini", url: "https://gemini.google.com/app?q=%s" },
      { id: "grok", name: "Grok", url: "https://grok.com/?q=%s" },
    ],
  },
  traditional: {
    name: "传统搜索", icon: "🔍",
    engines: [
      { id: "baidu", name: "百度", url: "https://www.baidu.com/s?wd=%s" },
      { id: "google", name: "Google", url: "https://www.google.com/search?q=%s" },
      { id: "bing", name: "必应 Bing", url: "https://www.bing.com/search?q=%s" },
      { id: "sogou", name: "搜狗", url: "https://www.sogou.com/web?query=%s" },
      { id: "duckduckgo", name: "DuckDuckGo", url: "https://duckduckgo.com/?q=%s" },
    ],
  },
  social: {
    name: "社交/内容", icon: "📱",
    engines: [
      { id: "xiaohongshu", name: "小红书", url: "https://www.xiaohongshu.com/search_result?keyword=%s" },
      { id: "douyin", name: "抖音", url: "https://www.douyin.com/search/%s" },
      { id: "weibo", name: "微博", url: "https://s.weibo.com/weibo?q=%s" },
      { id: "zhihu", name: "知乎", url: "https://www.zhihu.com/search?q=%s" },
      { id: "twitter", name: "推特 X", url: "https://twitter.com/search?q=%s" },
      { id: "reddit", name: "Reddit", url: "https://www.reddit.com/search/?q=%s" },
      { id: "facebook", name: "Facebook", url: "https://www.facebook.com/search/top/?q=%s" },
    ],
  },
  video: {
    name: "视频", icon: "🎥",
    engines: [
      { id: "bilibili", name: "哔哩哔哩", url: "https://search.bilibili.com/all?keyword=%s" },
      { id: "douyin_v", name: "抖音视频", url: "https://www.douyin.com/search/%s" },
      { id: "xigua", name: "西瓜视频", url: "https://www.ixigua.com/search/%s" },
      { id: "youtube", name: "YouTube", url: "https://www.youtube.com/results?search_query=%s" },
    ],
  },
  knowledge: {
    name: "百科/知识", icon: "📚",
    engines: [
      { id: "baike", name: "百度百科", url: "https://baike.baidu.com/search?word=%s" },
      { id: "zhihu_k", name: "知乎", url: "https://www.zhihu.com/search?q=%s" },
      { id: "wikipedia", name: "维基百科", url: "https://zh.wikipedia.org/wiki/Special:Search?search=%s" },
    ],
  },
  maps: {
    name: "地图", icon: "🗺️",
    engines: [
      { id: "amap", name: "高德地图", url: "https://www.amap.com/search?query=%s" },
      { id: "baidumap", name: "百度地图", url: "https://map.baidu.com/search?wd=%s" },
      { id: "qqmap", name: "腾讯地图", url: "https://map.qq.com/?what=%s" },
    ],
  },
};

export function faviconOf(url) {
  try { const h = new URL(url.replace("%s", "x")).hostname; return `https://www.google.com/s2/favicons?domain=${h}&sz=32`; }
  catch { return ""; }
}

// 展开成扁平清单：[{category, categoryName, id, name, url(已填入查询)}]
export function buildScan(query, { only } = {}) {
  const q = encodeURIComponent(query);
  const out = [];
  for (const [cat, group] of Object.entries(CHANNELS)) {
    if (only && !only.includes(cat)) continue;
    for (const e of group.engines) {
      out.push({ category: cat, categoryName: group.name, categoryIcon: group.icon, id: e.id, name: e.name, url: e.url.replace("%s", q), favicon: faviconOf(e.url) });
    }
  }
  return out;
}

export function channelCount() {
  return Object.values(CHANNELS).reduce((n, g) => n + g.engines.length, 0);
}
