/**
 * TUI 应用注册表 —— U-King 侧栏每个 AI 工具是独立应用。
 *
 * 一份数据驱动三处：① Sidebar 应用列表 ② App 路由（哪些 tab 是 TUI 应用）③ ToolAppView 的提示词按钮。
 * 每个应用进去是空终端 + 提示词按钮，点提示词才启动，启动后常驻保活，手动逐个关终端才停。
 *
 * 🔴 2026-10-21 工具目录收敛：「我的 AI」只保留 11 个工具（TOOL_SPECS 是唯一真相源）。
 * 这里只登记其中有终端入口的那批；mimo / codebuddy / qodercn / kimi / qwen / crush /
 * gemini / grok / cline / openclaw 的条目已删除，`node scripts/check-tool-registry-sync.mjs`
 * 保证本表与 TOOL_SPECS 的 id/cmd/config_target 一致。
 */

// 注意：codex 用 "codex-cli" 避免和现有 TabId 的 "codex"（Codex 专区）冲突。
// 「openclaw」仍留在联合类型里只为兼容存量代码（ToolAppView / SessionList 里还有它的比较
// 字面量），TUI_APPS 里已无该条目 —— OpenClaw CLI 按用户要求不出现在「我的 AI」。
export type TuiAppId = "claude" | "codex-cli" | "openclaw" | "hermes" | "dsh" | "opencode" | "pi";

export type TuiApp = {
  id: TuiAppId; // 路由 key + PTY tool tag
  toolId: string; // tools.rs 里的 ToolInfo.id（用于"已安装"过滤；注意 ≠ id）
  name: string; // 显示名
  tool: string; // 传 useTermGroup 的 tool tag（= id）
  /** 顶栏快捷提示词按钮：点了才在终端里跑（= 启动）。第一个 = 启动遮罩大按钮的主命令。 */
  prompts: { label: string; cmd: string }[];
  /** 启动遮罩大按钮文案（覆盖默认「启动 {name}」）。用于「主推网页版」这类需要点明的工具。 */
  launchLabel?: string;
  /** 切模型写哪个工具的配置（对齐 providers.rs apply_provider 的 target 字符串）。 */
  configTargets: string[];
  /** 左侧 Dock 分组：cli=命令行工具（默认）。GUI 应用不在这登记。 */
  group?: "cli";
  /** 隐藏入口（代码留着但不出现在 Dock/路由可达；未来插件容器复用）。 */
  hidden?: boolean;
  /** 启动时弹**独立系统终端窗口**（term_open_external）而不是挤进 U-King 内嵌终端。
   *  用于 Hermes 这类满屏 TUI —— 内嵌终端显示区域太窄（客户反馈）。ToolAppView 仍先
   *  ensureWebToolConfigured 配好虾盘云再开外部终端，规避「裸跑 hermes 不配 → 401」老坑。 */
  external?: boolean;
};

export const TUI_APPS: TuiApp[] = [
  {
    id: "claude",
    toolId: "claude-code",
    name: "Claude Code CLI",
    tool: "claude",
    prompts: [
      { label: "启动", cmd: "claude" },
      { label: "继续上次", cmd: "claude --resume" },
    ],
    configTargets: ["claude"],
    group: "cli",
  },
  {
    // ★ 2026-08-03 复活（此前 hidden=true，理由是「产品收窄到 5 工具，Codex 走桌面版专区」）。
    // 那条理由的前提已经反过来了：现在主推的是「CLI 干活 + U-Workspace 当壳」，桌面版才是
    // 被降级的一方。而 Codex 恰恰是**既能编程也能办公**的那一个，把它的 CLI 藏起来等于
    // 把最该主推的东西藏了。路由/保活/切驱动代码一直都在，这里只是把入口放回来。
    id: "codex-cli",
    toolId: "codex",
    name: "Codex CLI",
    tool: "codex",
    prompts: [{ label: "启动", cmd: "codex" }],
    configTargets: ["codex"],
    group: "cli",
  },
  {
    id: "hermes",
    toolId: "hermes",
    name: "Hermes Agent",
    tool: "hermes",
    // 主推「终端版」（2026-07-07 定，替代此前的网页版优先）：和 Claude Code 一样，点「启动」
    // 直接进 TUI 对话界面。配置在用户点「启动」那一下按需写好虾盘云（ToolAppView.
    // ensureWebToolConfigured，默认 deepseek-v4-flash），打开即能聊 —— 同一份 .env 也让其他
    // agent 能 `hermes -z` headless 互调。当年「外开终端裸跑 hermes 不配虾盘云」的坑不存在了。
    // 「网页版聊天」降为备选（hermes dashboard 起 127.0.0.1:9119 并自动开浏览器）。注意：
    // [web] extra 只代表 dashboard 可用；网页接管仍需 agent-browser 等，ToolAppView 单独体检。
    prompts: [
      { label: "启动", cmd: "hermes" },
      { label: "网页版聊天", cmd: "hermes dashboard" },
      { label: "重新初始化", cmd: "hermes setup" },
    ],
    configTargets: ["hermes"],
    group: "cli",
    // 主推：启动遮罩大按钮点明「干活利落」，呼应「我的 AI」里对 Hermes 的主推定位。
    launchLabel: "启动 Hermes 终端（推荐）",
    // 弹独立系统终端窗口跑 hermes（内嵌终端太窄挤，客户反馈；2026-07-09）。仍先配好虾盘云再开。
    external: true,
  },
  {
    // DSH 双入口：官方 Web 工作台 + dsh-terminal 插件提供的持续对话终端。
    // 两者都在独立 PTY 标签里常驻，避免把另一条启动命令写进已占住前台的进程。
    id: "dsh",
    toolId: "dsh",
    name: "DeepSeek Harness",
    tool: "dsh",
    prompts: [
      { label: "查看命令用法", cmd: "dsh --help" },
    ],
    launchLabel: "打开 DSH Web 工作台",
    // DSH rc.6 已有稳定 settings/credentials schema：U-King 只管自己的
    // uking-managed route，Web / terminal 共用同一份 `$DSH_HOME`。
    configTargets: ["dsh"],
    group: "cli",
  },
  {
    // ★ 2026-08-03 上架，但**只当交互式 TUI 用**（★192k，社区最大的开源 coding agent）。
    // 🔴 它的非交互入口 `opencode run` 在本机三轮实测**恒定挂满超时、零字节输出**：
    // 1.4.3 沙箱挂 90s → 升 1.18.11 挂 100s → **真实 HOME + 用户自己配好且缓存过的
    // provider (`-m z/glm-5`) 仍挂 100s** → 去掉 --format 裸跑挂 45s。第三条是决定性的：
    // 排除了「我们的配置 / 沙箱 / 版本」三种解释，是它 run 子命令本身在这台 Windows 上起不来。
    // 所以这里**故意不给非交互提示词**，竞技场也不带它 —— 带一个必挂的选手去比，比出来的
    // 不是「谁干活利索」，是「我们的跑道有 bug」。哪天上游修好了，加个 prompts 项即可。
    id: "opencode",
    toolId: "opencode",
    name: "OpenCode",
    tool: "opencode",
    prompts: [
      { label: "启动", cmd: "opencode" },
      { label: "继续上次", cmd: "opencode --continue" },
    ],
    configTargets: ["opencode"],
    group: "cli",
    // ★ 2026-08-24 取消隐藏（用户点名要 opencode 出现在「我的 AI」和「AI 设置」里，
    // 同批把 `tools.rs` 的 hidden 也翻了）。「装得慢」是安装时才付的代价，
    // 换来「装完在工作台 Dock 里也找不到它」不划算。
    hidden: false,
  },
  {
    // ★ 2026-08-03 上架。四条门槛本机实测**全过**（用便携 Node 22.20 + npmmirror + 沙箱 HOME）：
    //   ① npm 131 包 / 29s；落盘 153MB / 18718 个文件（比 Qwen、Crush 大一档，慢网要多等）
    //   ② `~/.pi/agent/models.json` 接虾盘云，4.3s 回话；默认模型另写 settings.json 的
    //      `defaultModel`（不写它，pi 默认 provider 是 google，客户敲 `pi` 会撞交互式登录）
    //   ③ `pi -p` exit 0、stdout 只有结果、stderr 0 字节；`--mode json` 是逐行事件流，
    //      带 usage/cost —— 竞技场要的可观测量它自己就给了，不用扒 stdout
    //   ④ 工具调用真跑：只读档 `--tools read` 4s 读到文件；写档 7.4s 真落盘
    // 编程能力实测（同 flash 模型对照 Claude Code）：阶梯累进计费 3 个 bug，
    // pi 22.8s 全改对、test.js md5 一字未动；Claude Code 39.1s 也过。**能力看模型，壳决定快慢**。
    // 为什么快：同任务同模型，pi 塞 5,000 token 上下文，Claude Code 塞 24,300 —— 差 4.8 倍。
    // 🔴 它自己**没有权限系统**（README 明说），靠 `--tools/--exclude-tools/--no-tools` 白名单兜。
    // 🔴 要 Node ≥22.19：skill 里那步 ensure_node 带 `min`，系统 Node 太旧会改用便携版。
    id: "pi",
    toolId: "pi",
    name: "pi",
    tool: "pi",
    prompts: [
      { label: "启动", cmd: "pi" },
      { label: "继续上次", cmd: "pi --continue" },
      { label: "只读模式", cmd: "pi --tools read" },
    ],
    configTargets: ["pi"],
    group: "cli",
  },
];

/** 兼容旧入口的可见应用列表；生产启动入口由后端工具目录决定。 */
export const VISIBLE_TUI_APPS = TUI_APPS.filter((a) => !a.hidden);

export const TUI_APP_IDS: TuiAppId[] = TUI_APPS.map((a) => a.id);

export const tuiAppById = (id: string): TuiApp | undefined => TUI_APPS.find((a) => a.id === id);

export const isTuiAppId = (id: string): id is TuiAppId => (TUI_APP_IDS as string[]).includes(id);
