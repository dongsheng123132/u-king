# 任务交接 · B 极简管家首页 + 软件库（2026-10-22）

> 用户口径（原话）：「b极简，我要省token，不要的要删除。软件要能加，能分类……比如要加 magpie，
> 能加上，能装，不行就丢提示词进 uking 轻助手，让ai装。其他你看着办，这个东西能减的要减，要增强的增强。」
>
> 同一天的上一条口径（看图/token）：「说你好还是用 4.2 万 token，主要是输入是什么……终端是不是也消耗这么多」——
> 详见本文件「每轮底座账单」一节，那是这一版新增能力的来由。

## 一句话

「我的 AI」从瓷砖墙换成 **B 极简首页**（一句话状态 → 四个动作块 → 已装细线列表 → 推荐装 →
每轮底座账单）；新增**软件库**页（分类 + 搜索 + 一键安装，装不上可「让 AI 装」）；
**magpie 重新上架**（可见、可装）；后端新增只读 **`context_bill`**，把「每轮底座」从黑盒变成
可看的数字；旧 ToolHub 瓷砖墙下线（文件先留，回退成本 = 改 App.tsx 三行）。

## 落点

| 文件 | 改动 |
| --- | --- |
| `src/home/HomePage.tsx`（新） | B 极简首页：一句话状态 + 四动作块（体检 / 软件库 / 省额度 / 换模型·余额）+ 已装细线列表（点开就地展开：换模型 / 升级修复 / 让 AI 修 / 卸载）+ 推荐装 + 每轮底座账单（`invoke("context_bill")`，只读）+ 终端快照重开条 |
| `src/library/LibraryPage.tsx`（新） | 软件库：分类（全部 / 智能体 / 桌面应用 / 命令行）+ 搜索（名称/用途/id）+ 安装；每个可装工具都带「让 AI 装」→ 脱敏维护任务包填进轻助手草稿（不自动发送）；已装的显示启动/打开 |
| `src/lib/toolMeta.ts`（新） | 分类映射 + 推荐排序的唯一一份（首页、软件库共用；不许各写一套） |
| `src/lib/toolPrompt.ts`（新） | 「让 AI 装 / 让 AI 修」任务包：复用 `buildMaintenanceTask`（同一份脱敏规则），不另写模板 |
| `src/App.tsx` | 新页懒加载；`toolhub` tab 渲染 HomePage；新增 `library` tab 渲染 LibraryPage；旧 ToolHub 不再 import（文件保留一轮） |
| `src/components/Sidebar.tsx` | `TabId` 加 `library`；CORE 在「我的 AI」后加「软件库」 |
| `src/i18n/en/home.ts`（新）、`src/i18n/en.ts` | 新增文案英文覆盖（含 `t(变量)` 扫不到的动态 key：记忆文件名、分类名、侧栏副标题） |
| `src-tauri/src/context_bill.rs`（新） | 只读账单：常量 `CLAUDE_BASE_TOKENS=20700`（2026-10-22 实测 Claude Code 2.1.302）+ `.claude/skills` / `.claude/skills-off` / `.uking/skills` 三个技能目录（含 `SKILL.md` 的一级子目录数 + Unicode 字符数）+ 三个记忆文件体积；家目录走 `installer::user_home_dir()`（认沙箱）；目录不存在/读失败一律 0/空，不 panic；4 个沙箱单测 |
| `src-tauri/src/tools.rs` | magpie `hidden: true → false`（重新上架），注释同步，补「装/未装都必须可见」守卫断言；TOOL_SPECS 数据字段未动 |
| `src-tauri/src/lib.rs` | `mod context_bill;` + `generate_handler` 注册（命令名 `context_bill`，无参） |
| `docs/HANDOFF-20261021-workbench-restore.md` | 顺手修一条泄漏闸门 medium：把一处内部巡检工具名改成「巡检 / 验证类包」 |

## 用户口径逐条落点

- **B 极简**：页面上只有主按钮/动作块有块面，其余全是文字 + 1px 细线；已装工具的操作点开才出现。
- **省 token**：首页账单条 + 展开明细：Claude Code 本体（约 2.07 万 tokens，实测常量）、技能包
  个数与说明字符数、三个记忆文件体积，并给出三条省法（卸技能包 / 开新对话 / 精简全局记忆）。
- **软件能加、能分类**：软件库分类 + 搜索；「安装」走既有装机向导（`onInstallSelection`），
  不新开安装实现。
- **magpie**：`hidden=false` 后随 `list_tools` 露出；安装项本来就在装机清单里（v65，单文件
  `magpie-cli-windows-amd64.exe` + SHA-256 钉死），点安装 = 走向导；装不上点「让 AI 装」。
- **不行就丢提示词进轻助手**：`buildAiInstallPrompt` / `buildAiRepairPrompt` 生成统一脱敏任务包，
  经 App.tsx `pendingChatPrompt` 填进维护助手草稿（用户按发送才走）。
- **能减就减**：旧 ToolHub（瓷砖墙 + 详情条 + 多选安装 + 实验室分组）不再被引用、不进打包；
  首页动作只留四个；列表操作收起。
- **该增强增强**：安装/修复失败有 AI 兜底入口；账单不猜账；分类/推荐只有一份真相源。

## 验证（2026-10-22，本机）

- `pnpm build` → **exit 0**（模块耦合 / 主题 / i18n 漏翻 / 技能清单 / 模型目录 / 供应商模板 /
  工具注册表 / tsc / vite / webkit 兼容全过）。
- `cd src-tauri && cargo check` → **exit 0**。
- `cargo test --lib context_bill` → 4 passed / 0 failed。
- `cargo test --lib`（默认并行）→ 605 passed / 1 failed：唯一失败是**既有偶发**
  `agent::chat::tests::curl_config_streams_sse_and_sends_body_over_loopback`（curl 56 RST，
  HANDOFF-20261021 已记录）；`--test-threads=1` 全量 → **606 passed / 0 failed / 3 ignored**。
- `node scripts/check-leak.mjs .` → **PASSED (0 hits)**。
- `pnpm tauri build` → 见下方「产物」（本机构建）。
- 启动冒烟：见「产物」一节（进程起来、窗口出现；本机不截屏做 UI 验收）。

## 未做 / 待拍板

1. **真机 UI 验收**：B 版式在绿色版上的实际观感（间距、窄屏、深浅色两套主题）——本机不截屏验，
   需要在干净机/你自己电脑上看一遍。
2. **省额度「一键瘦身」**（下一版最该做的）：技能停用（移到 `skills-off`，可一键开回）、
   记忆文件只读查看/清理建议。本轮只展示账单，**一个用户文件都没动**。
3. **更新中心**：逐工具「已装版本 vs 最新版本」比对 + 一键全升。现在首页的「体检」仍跳
   「体检 · 升级」页，单工具升级 = 详情里「升级 / 修复（重走安装）」。
4. **删旧 ToolHub**：`src/toolhub/ToolHub.tsx` + `src/i18n/en/toolhub.ts` 的词条先留着（先藏后摘），
   等真机确认新首页没丢功能再删；届时 App.tsx 只需保持现状。
5. **分类来源**：前端按 `kind`/`launch_app` 推的展示分类；后端以后补 `launch_mode` 应直接切过去。
6. 版本号未动（仍是 1.4.9，本次未发版）。
