/**
 * TUI 应用注册表 —— U-King 侧栏每个 AI 工具是独立应用。
 *
 * 一份数据驱动三处：① Sidebar 应用列表 ② App 路由（哪些 tab 是 TUI 应用）③ ToolAppView 的提示词按钮。
 * 每个应用进去是空终端 + 提示词按钮，点提示词才启动，启动后常驻保活，手动逐个关终端才停。
 */

// 注意：codex 用 "codex-cli" 避免和现有 TabId 的 "codex"（Codex 专区）冲突
export type TuiAppId = "claude" | "codex-cli" | "openclaw" | "hermes" | "dsh" | "qwen" | "crush" | "opencode" | "pi" | "cline";

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
    // ★ 2026-08-03 复活。原隐藏理由（2026-07-07）是「ClawX 桌面版就是 OpenClaw 的 GUI 版，
    // 双入口让客户犯懵」—— 在「GUI 优先」的前提下成立，但现在 ClawX 自己被降级了，
    // 于是这条理由连同它的前提一起没了：**唯一的人类入口反而应该是 CLI**。
    // 双配置面（openclaw.json vs clawx-providers.json）和 18789 端口冲突那两个老坑仍然真实，
    // 靠的是 configTargets 仍指 "clawx"（同一份配置），不是靠藏入口。
    id: "openclaw",
    toolId: "openclaw",
    name: "OpenClaw CLI",
    tool: "openclaw",
    // gateway 必须带 --allow-unconfigured，否则裸 `openclaw gateway run` 会报
    // 「Missing config. Run openclaw setup...」直接卡住（客户实测懵逼点）。
    prompts: [
      { label: "启动网页版", cmd: "openclaw gateway run --allow-unconfigured --port 18789" },
      { label: "命令行版", cmd: "openclaw" },
    ],
    configTargets: ["clawx"], // 注意：openclaw 的 apply_provider target 是 "clawx" 不是 "openclaw"
    group: "cli",
    // ★ 2026-08-05 重新隐藏（第三次翻转，所以这次把判据写死）。
    // **隐藏的是入口，不是砍配置能力** —— gateway / 备份 / configTargets("clawx")
    // 那条链一个字节没动，已装的客户照配照用。
    // ⚠️ 但要清楚这一改的合力：ClawX 桌面版（tools.rs 里另一条 id）08-03 已降级隐藏，
    // 所以 **OpenClaw 生态在主推面上从此不再出现**，只在「进阶 / App 版」页可达。
    // 这是有意的收窄；哪天要复活，两处得一起放回来，只放一处客户仍然点不通。
    // 判据（不是"不好用"，它好不好用这里根本没测出来）：
    //   · 它**没有可靠的无头一次性推理入口**：`infer model run` 走另一套模型目录，
    //     显式传 --model 直接 Unknown model，而我们写的三份配置一个字段都没错。
    //     在「CLI 干活 + U-Workspace 当壳」的主线里，一个脚本调不动的 CLI 价值有限。
    //   · 双入口（ClawX 桌面版 + 这个）让客户犯懵 —— 这是 2026-07-07 隐藏它的原始理由，
    //     08-03 以「ClawX 被降级、唯一人类入口应该是 CLI」复活；但那轮复活没解决
    //     「gateway 起不来就什么都没有」，客户看到的仍是两个都点不通。
    hidden: true,
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
    // ★ 2026-08-03 新上架。**本机实测过才写进来的**（口径见下），不是照 awesome-list 抄的 ——
    // 同一轮筛选里 PyPI 上的 `goose-ai` 是抢注空包、OpenCode 1.4.3 的 `run` 在干净 HOME 下
    // 挂 90 秒零输出，两个都没进来。上架四条硬门槛，缺一条就不上：
    //   ① 装得上：npm 12 个包 / 19s（npmmirror 源）
    //   ② 接得上虾盘云：`~/.qwen/settings.json` 的 modelProviders.openai[]（形状取自包内
    //      自带的 qc-helper/docs/configuration/auth.md，权威来源，不是猜的）
    //   ③ 非交互塞得进任务：`qwen -p "…"` → exit 0 / 12.5s / **stdout 只有答案、stderr 空**
    //   ④ 工具调用真能跑：让它读文件，默认审批档就通过，不用 --yolo（8.8s 拿到真内容）
    // ③ 那条是竞技场的地基：stdout 干净才解析得动「谁干活利索」。
    id: "qwen",
    toolId: "qwen-code",
    name: "Qwen Code",
    tool: "qwen",
    prompts: [
      { label: "启动", cmd: "qwen" },
      { label: "继续上次", cmd: "qwen --continue" },
    ],
    configTargets: ["qwen"],
    group: "cli",
    // ★ 2026-08-05 隐藏。**不是因为它坏** —— 沙箱实测（U-King 配好的虾盘云）真回话。
    // 是因为慢：35.7s，全场最慢的一档（pi 7.1s / claude 9.6s / crush 12.1s）。
    // 主推线是「Claude Code + Hermes」（0.9.88 定），它既不快也不差异化，占一格 Dock
    // 只会让客户多一次「该选哪个」的犹豫。apply_qwen 保留：已装的客户照样自动配上。
    hidden: true,
  },
  {
    // ★ 2026-08-03 新上架，同上四条门槛实测：
    //   ① npm 48 包 / 7s（内含 Go 单二进制，装完 `crush version` = v0.88.0）
    //   ② 虾盘云走 crush.json 的 providers + **models.large/small 必须显式指过去** ——
    //      只写 providers 不写 models，它会拿 OPENAI_API_KEY 去打 api.openai.com 报
    //      「Incorrect API key」，看起来像我们的 Key 坏了，其实是根本没走我们的端点。
    //      这个坑第一次配就踩到了，别再让客户踩。
    //   ③ `crush run "…"` → exit 0 / 8.4s / stdout 干净 / stderr 空，且支持管道
    //   ④ 只读工具调用 exit 0 / 9.4s
    id: "crush",
    toolId: "crush",
    name: "Crush",
    tool: "crush",
    prompts: [
      { label: "启动", cmd: "crush" },
      { label: "继续上次", cmd: "crush --continue" },
    ],
    configTargets: ["crush"],
    group: "cli",
    // ★ 2026-08-05 隐藏。要说清楚的是：**它现在是能用的** —— 上面②那条注释当年没查到
    // 真正的坑，真因是配置写错了目录（我们写 ~/.config/crush，crush v0.88 在 Windows 读
    // %LOCALAPPDATA%\crush），2026-08-04 已修并实测 12.1s 通过。
    // 隐藏的理由是重叠而非故障：和 Claude Code / Codex 干同一件事，没有差异化价值。
    // 代码和 apply_crush 全部保留 —— 已装的客户升级后配置照旧生效，不会突然坏。
    hidden: true,
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
    // ★ 2026-08-05 隐藏。**上面那段「run 恒定挂满超时」的结论要修正**：2026-08-04 用
    // U-King 配好的虾盘云（deepseek-v4-flash）在沙箱里跑通了，11.8s 有输出。
    // 所以那三轮挂大概率是 provider 的锅（当时真实 HOME 里配的是 zhipuai glm-4.6），
    // 不是它 run 子命令本身起不来 —— 原注释把「换个 provider 就好」写成了「它坏了」。
    // 2026-08-24 取消隐藏（用户点名要 opencode 出现在「我的 AI」和「AI 设置」里，
    // 同批把 `tools.rs` 的 hidden 也翻了）。「装得慢」是安装时才付的代价，
    // 换来「装完在工作台 Dock 里也找不到它」不划算 —— 上面那条修正本身就说明
    // 当初的隐藏理由（run 子命令坏了）是误判，剩下的只有体积，而体积已在文案里明说。
    hidden: false,
  },
  {
    // ★ 2026-08-29 上架（会审裁定：opus + gpt-5.6-sol 双路）。上架四门槛 2026-08-29 本机
    // 沙箱实测全过（npm 328 包/17s；`cline auth openai-compatible` 非交互接虾盘云 exit 0；
    // `cline --json` 无头真调 6.6s 回话；工具调用 34.1s 真改文件并跑 node 验证）。
    // 差异化 = headless NDJSON 事件流 + schedule/cron + kanban + --worktree（自动化编排线），
    // 与 OpenCode 的「交互 TUI」定位区分。体量大：328MB/27769 文件，慢网要等（文案已写明）。
    // 🔴 两个实测坑写死在这：
    //   ① 单 token prompt（"OK"/"你好"）被 commander 当未知子命令拒绝 —— prompts 一律多词；
    //   ② 裸跑没配 provider 报 "Unauthorized ... re-authenticate" —— ensureWebToolConfigured
    //      已把 cline 纳入（ToolAppView），先配虾盘云再启动。
    //   ③ 官方没有 --resume 旗标（sol 实锤 `unknown option '--resume'`），继续上次用 history。
    id: "cline",
    toolId: "cline",
    name: "Cline CLI",
    tool: "cline",
    prompts: [
      { label: "启动", cmd: "cline" },
      { label: "历史会话", cmd: "cline history" },
      { label: "一次性任务", cmd: "cline 说说你能做什么" },
    ],
    configTargets: ["cline"],
    group: "cli",
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

/** 可见 TUI 应用（Dock + 路由用这个，过滤掉 hidden 的 codex-cli）。 */
export const VISIBLE_TUI_APPS = TUI_APPS.filter((a) => !a.hidden);

export const TUI_APP_IDS: TuiAppId[] = TUI_APPS.map((a) => a.id);

export const tuiAppById = (id: string): TuiApp | undefined => TUI_APPS.find((a) => a.id === id);

export const isTuiAppId = (id: string): id is TuiAppId => (TUI_APP_IDS as string[]).includes(id);
