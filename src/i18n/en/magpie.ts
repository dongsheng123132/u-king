/** 「AI 设置 → 路由」分区（`components/MagpieRoutePanel.tsx`）+ 分区条那一行（`Manager.tsx`）的英文词条。
 *  2026-11-05 1.4.12 magpie 路由批次。与别处共用的词（刷新 / 还原 / 未配置 / 未安装）复用中央字典，不重复翻。
 *  🔴 「读取不到当前模型」是**单独一条**、不是「未配置」的另一种译法 —— 前者是「不知道」，
 *     后者是「确认没配」，混成一个词就把未知画成了结论。 */
export const magpie: Record<string, string> = {
  // ── 分区条（Manager.tsx）────────────────────────────────────
  "路由": "Routing",
  "让 magpie 把多个订阅合成一路，按任务自动挑模型":
    "Let magpie merge subscriptions into one route and pick a model per task",

  // ── 面板骨架 / 状态 1：读取中 ────────────────────────────────
  "magpie 路由": "magpie routes",
  "magpie 把多份订阅合成一路，按任务自动挑模型；这里决定每个 AI 用哪一路。":
    "magpie merges several subscriptions into one route and picks a model per task. Here you choose which route each AI uses.",
  "正在读取 magpie 路由…": "Reading magpie routes…",
  "读取路由失败：": "Could not read routes: ",
  "重新读取": "Reload",
  "运行中": "Running",
  "没在运行": "Not running",

  // ── 状态 2：没装 magpie ──────────────────────────────────────
  "magpie 是本地模型管家：把多份订阅、多个 Key 合成「路由组」，AI 工具把它当成一个模型来用。":
    "magpie is a local model manager: it merges several subscriptions and keys into route groups, and AI tools treat a group as a single model name.",
  "先去「我的 AI」安装 magpie": "Install magpie in My AI first",
  "装好并启动 magpie 后，回这里就能选路由。":
    "Once magpie is installed and running, come back here to pick routes.",

  // ── 状态 3：装了没在跑 / 状态 4：没有路由组 ────────────────────
  "magpie 已安装但没在运行，路由要它开着才生效":
    "magpie is installed but not running. Routes only work while it is running.",
  "打开 magpie": "Open magpie",
  "打开 magpie 失败：": "Could not open magpie: ",
  "已请 magpie 打开窗口": "Asked magpie to open its window",
  "magpie 里还没有路由组。到 magpie 的 Routing 面板建一个，回来点刷新。":
    "No route groups in magpie yet. Create one in magpie's Routing panel, then come back and refresh.",
  "magpie 报了个错：": "magpie reported an error: ",

  // ── 状态 5：路由组 ──────────────────────────────────────────
  "我的路由组": "My route groups",
  "一组由你配的多家模型组成，AI 工具看到的只是这一组的名字。":
    "A group you build from several providers; AI tools only see the group's name.",
  "还没有自己建的路由组。": "No route groups of your own yet.",
  "读自 magpie 的本机配置（没连上它的网关）":
    "Read from magpie's local config (its gateway is not reachable)",
  "自动路由": "Automatic routes",
  "同一个模型多家兜底，数量多，默认收起":
    "Several providers back each model; there are many, so this stays collapsed.",

  // ── 状态 5：五工具切换表 ─────────────────────────────────────
  "让哪个 AI 用哪一路": "Which AI uses which route",
  "选一路再点「切到这一路」；想恢复原样点「还原」。":
    "Pick a route, then click Switch to this route. Use Reset to go back to the original config.",
  "没装也能先配好，装上后直接生效":
    "You can set this up before installing — it takes effect once installed.",
  "读取不到当前模型": "Can't read the current model",
  "当前：{model}（不是 magpie 路由）": "Current: {model} (not a magpie route)",
  "选择一路…": "Pick a route…",
  "切到这一路": "Switch to this route",
  "还原成它原来的配置": "Reset it back to its original configuration",
  "没读到可切换的 AI，点刷新再试。": "No switchable AI was returned. Refresh and try again.",
  "切换由 magpie 写进各 AI 自己的配置；切完要重启那个 AI 工具才看得到新模型。":
    "magpie writes the switch into each AI tool's own config. Restart that tool to see the new model.",

  // ── toast ─────────────────────────────────────────────────
  "已切到 {name}": "Switched to {name}",
  "切换失败：": "Switch failed: ",
  "已还原": "Reset to the original config",
  "还原失败：": "Reset failed: ",
};
