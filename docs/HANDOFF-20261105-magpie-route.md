# 交接：AI 设置新增「路由」分区（magpie 路由选项）

> 目标版本：1.4.12（本文件是这一版的功能设计稿；实现完成后本文件留档，不删）。
> 用户口径（2026-11-05）：「下一个 uking 版本……AI 设置里边加上 magpie 的路由选项」。

## 1. 要解决的事

magpie（yetone/magpie，本地模型管家）会把用户的多份订阅 / 多个 Key 合成「路由组」，
组对 AI 工具来说就是一个模型名：`group/<id>`（自家路由组）或 `auto/<模型>`（同一个模型多家
provider 兜底）。magpie 用 CLI 把某个 AI 工具的模型切到某一路：

```
magpie <agent> group/<id>      # 切过去（写该工具自己的配置）
magpie <agent> default         # 还原成它原来的配置
magpie <agent> model           # 回读：「Claude Code model group/smart-split」
```

U-King 现在已经能装 magpie（「我的 AI」里，官方桌面版），但装完之后**在 U-King 里看不见它
的路由**，客户得离开 U-King 去 magpie 窗口里点。这一版把「路由」搬进 AI 设置。

范围（本版**只做**这五条，不多做）：

| U-King 配置目标 | magpie agent | 说明 |
| --- | --- | --- |
| claude | claude | Claude Code（工作台同用一份 `~/.claude/settings.json`） |
| codex | codex | Codex CLI / 桌面 App |
| dsh | dsh | DeepSeek Harness |
| pi | pi | Pi |
| hermes | hermes | Hermes Agent |

ClawX / OpenCode 本版不做行（magpie 没有对应 agent 或口径不明），面板里不出现、也不假装支持。

## 2. 数据来源（全部本机，不出网）

| 要的东西 | 从哪读 | 备注 |
| --- | --- | --- |
| magpie 装没装 | magpie 可执行文件 | Windows：`%USERPROFILE%\.uking\apps\magpie\magpie.exe`（U-King 自己的落点）、`%LOCALAPPDATA%\Programs\magpie\magpie.exe`、PATH；macOS：`/Applications/Magpie.app/Contents/MacOS/magpie`、`~/Applications/...`、PATH |
| 网关在不在跑 | TCP 连 `127.0.0.1:3425`（200ms 超时） | magpie 的运行态网关 |
| 路由组（在线） | `GET http://127.0.0.1:3425/v1/models` | 只取 `id` 以 `group/` / `auto/` 开头；`display_name` 当人话名；`reasoning_levels` 当档位 |
| 路由组（离线兜底） | `~/.config/magpie/providers.json` 的 `groups[]` | Windows 候选 `%APPDATA%\magpie\providers.json` / `%LOCALAPPDATA%\magpie\providers.json`。`{id,name,members,routing}` |
| 某个 AI 当前走哪一路 | `magpie <agent> model`，取**最后一个空白分隔 token** | `magpie/group/x` 与 `group/x` 都要认（不同 agent 前缀不同） |

**HTTP 一律走 `crate::installer::curl(...)`**（仓库纪律：系统 curl + 不新增 HTTP 依赖），
并且必须带 `--noproxy *` —— 本机回环不能被系统代理劫持。

## 3. 写入（切换 / 还原）

- 切换：`<magpie exe> <agent> <model>`，`model` 只接受 `group/*` 或 `auto/*`（先在 Rust 校验，
  不合法直接拒绝，不 spawn）。
- 还原：`<magpie exe> <agent> default`。
- 统一 `std::process::Command` + `installer::output_with_timeout`（25s）+ Windows
  `creation_flags(0x0800_0000)`（CREATE_NO_WINDOW，防闪黑窗）。
- 写完**必须回读**：再跑一次 `magpie <agent> model`，等于目标值才算成功；
  回读不一致 → 返回 `Err`，把「magpie 说成功、回读是 X」如实告诉客户（不许把没生效说成生效）。
- CLI 失败信息只回传尾部 200 字符（人话优先），不落盘、不写日志文件。

## 4. 后端契约（新增模块 `src-tauri/src/magpie_route.rs` + 4 个 command）

> 🔴 耦合纪律：`magpie_route.rs` **只能依赖公共层**（`installer` / `fs` / `ulog`）。
> 需要 `providers::effective_config` 这类功能模块能力时，**由组合根 `lib.rs` 取好、当参数传进来**
> （`check-module-coupling.mjs` 会拦新增耦合）。

```rust
// 模块导出（形状，不是最终代码）
pub const AGENTS: &[(&str /*target*/, &str /*agent*/, &str /*label*/)];

#[derive(Serialize, Clone, Default)]
pub struct RouteOption {
    pub model: String,        // "group/smart-split" | "auto/deepseek-v4.1-flash"
    pub name: String,         // display_name，缺省用 id
    pub kind: String,         // "group" | "auto"
    pub levels: Vec<String>,  // reasoning_levels（没有就空）
    pub members: Vec<String>, // providers.json 有就带；网关来源为空
}

#[derive(Serialize, Clone, Default)]
pub struct ToolRoute {
    pub target: String,       // claude / codex / dsh / pi / hermes
    pub label: String,        // Claude Code / Codex / …
    pub agent: String,        // magpie agent id
    pub model: Option<String>,// CLI 回读的原始值（已剥 magpie/ 前缀）
    pub route: Option<String>,// 是路由就是 "group/x" / "auto/x"，否则 None
    pub source: String,       // "cli" | "config" | "none"
    pub readable: bool,       // false = 不知道，UI 不许渲染成「没配」
}

#[derive(Serialize, Clone, Default)]
pub struct RouteState {
    pub installed: bool,
    pub running: bool,
    pub gateway: String,      // "http://127.0.0.1:3425"
    pub routes: Vec<RouteOption>,
    pub tools: Vec<ToolRoute>,
    pub routes_source: String,// "gateway" | "config" | "none"
    pub error: Option<String>,
}

pub fn magpie_exe() -> Option<PathBuf>;
pub fn running() -> bool;
pub fn read_agent_model(agent: &str) -> Option<String>;        // CLI
pub fn apply(agent: &str, model: &str) -> Result<(), String>;
pub fn reset(agent: &str) -> Result<(), String>;
pub fn open_app() -> Result<(), String>;                       // 直接拉起 magpie 窗口
pub fn state(cfg: &[(String, Option<String>, Option<String>)]) -> RouteState; // (target, fallback_model, fallback_base)
```

纯函数（**必须有单测**，不许依赖本机装没装 magpie）：

- `parse_agent_model(label: &str, out: &str) -> Option<String>`：从 `"Claude Code model group/x"` 取 `group/x`。
- `normalize_route(model: &str) -> Option<String>`：剥 `magpie/`，只认 `group/` `auto/` 开头。
- `parse_gateway_models(json: &str) -> Vec<RouteOption>`：`/v1/models` 体 → 只留 group/auto，group 排前面。
- `parse_config_groups(json: &str) -> Vec<RouteOption>`：`providers.json` → groups[]。
- `pick_exe(found: Vec<Option<PathBuf>>) -> Option<PathBuf>`：候选探测注入式（测试用）。
- `assemble(installed, running, routes, routes_source, tools, error) -> RouteState`。

四条 command（`lib.rs` 里薄封装，直接进 `generate_handler!`，**不做 ActionParity 动作**——
它们不是「影核动作」，只是设置页的读写）：

| command | 入参 | 返回 |
| --- | --- | --- |
| `magpie_route_state` | 无 | `RouteState` |
| `magpie_route_apply` | `target: String, model: String` | `Result<RouteState, String>` |
| `magpie_route_reset` | `target: String` | `Result<RouteState, String>` |
| `magpie_route_open` | 无 | `Result<(), String>` |

`magpie_route_state` 里由 `lib.rs` 取 `providers::effective_config(target)` 的 `model` / `base_url`
当**兜底**（CLI 读不到时用：`base_url` 命中 `127.0.0.1:3425` 就说明确实在走 magpie 路由，
但具体哪一路读不到就 `route=None` + `readable=false`，不许猜）。

## 5. 前端契约（`src/components/MagpieRoutePanel.tsx` + Manager 接入）

分区：`SettingsTab` 增加 `"route"`；分区条位置在「工具分配」之后（配 → 路由 → 供应商库 → …），
label `路由`，hint `让 magpie 把多个订阅合成一路，按任务自动挑模型`，图标 `Route`（lucide-react）。

面板状态机（**每一态都要有文案，不能空白**）：

1. `state === null` → 「正在读取 magpie 路由…」
2. `installed === false` → 说明卡：magpie 是什么（一句话）+「先去‘我的 AI’安装 magpie」
   （`onGoPage?.("toolhub")` 跳过去）+ 本面板其余内容不渲染。
3. `installed && !running` → 黄色提示条：「magpie 已安装但没在运行，路由要它开着才生效」+
   「打开 magpie」按钮（`invoke("magpie_route_open")`）+ 仍然展示路由组（离线来源）与工具行。
4. `routes.length === 0` → 「magpie 里还没有路由组。到 magpie 的 Routing 面板建一个，回来点刷新」+
   「打开 magpie」「刷新」。
5. 正常态：
   - 「我的路由组」（kind=group）卡片网格：名字（`name`）、`model` id、档位 chips；
   - 「自动路由」（kind=auto）默认折叠（数量可能几十个），展开才显示，卡片同款；
   - 「让哪个 AI 用哪一路」表格：五行（claude/codex/dsh/pi/hermes），每行
     - 工具名 + 未安装徽章（`tools` prop 里 `installed === false` 时给「未安装」提示，**不禁用**——
       先配好再装也是合法路径；只提示）
     - 当前：`route` 有值 → 「→ {组名 / model}」；否则 `readable` 为 false → 「读取不到当前模型」；
       有 `model` → 「当前：{model}（不是 magpie 路由）」；再否则「未配置」
     - 选择（`<select>`，选项 = 全部 routes，`group/...` 分组在前）
     - 「切到这一路」按钮（调 `magpie_route_apply`）
     - 已有路由时给「还原」按钮（调 `magpie_route_reset`）
   - 底部一句话：切换由 magpie 写进各 AI 工具的配置；切换后**重启那个 AI 工具**才看得到新模型。
6. 每次 apply/reset 成功后用返回的 `RouteState` 直接替换本地状态（不再多发一次 state 调用），
   并 toast：「已切到 X」/「已还原」。

其它前端纪律：

- 文案一律 `t("中文")` 包裹，英文词典放**新文件** `src/i18n/en/magpie.ts`，在
  `src/i18n/en/index.ts` 里 import + spread（`check-i18n-missing.mjs` 会查漏）。
- 不新增依赖；只用现有 `cn` / `useI18n` / `invoke` 与既有卡片视觉（`rounded-card border border-white/[0.08] bg-bg-1/70`）。
- Hook 无条件调用；`useEffect` 里 `alive` 守卫，与 Manager 里其它异步一致。
- Manager.tsx 改动只允许：`SettingsTab` union 加 `"route"`、分区条数组加一行、渲染块加一行
  `<MagpieRoutePanel … />`、`tools` prop 透传。**不要动已有分区代码。**

## 6. 验收

- 后端：`cd src-tauri && cargo test --lib magpie_route` 全绿；`cargo check` 无新 warning。
- 前端：`pnpm build` 全绿（含 i18n / 主题 / 模块耦合 / webkit 兼容检查）。
- 耦合：`node scripts/check-module-coupling.mjs` 维持基线（39 条，不许涨）。
- 泄漏：`node scripts/check-leak.mjs .` 0 命中（新增文件里不得出现真实用户名 / Key / 客户机号）。
- 真机（macOS 本机）：magpie 在跑 → 面板能列出 6 个 group + 一排 auto；`magpie dsh` 当前
  `magpie/group/smart-split` 要在 DSH 行如实显示（这一条最能证明回读真的读了别人的配置）；
  切一路再还原，配置文件被 magpie 写回（可对照 `~/.pi/agent/settings.json` 这类文件确认）。
