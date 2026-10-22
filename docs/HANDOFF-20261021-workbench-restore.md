# 任务交接 · 工作台回调 + 工具目录收敛（2026-10-21）

> 本文记录「10-20 那版精简工作台」被用户回调后的落点。改动全部在工作树，**未提交、未发版**。
> 分支 `feat/slim-maintenance-assistant`，基 main（比 main 多 4 个提交，即已发布的 1.4.0）。

## 一句话

工作台回到 1.4.0 之前的原样（**U-Chat 底座 Claude Code + 终端 + 会话列表**），维护助手退回维护场景；
「我的 AI」工具目录收敛到 11 项（magpie 回到这里）；侧栏页脚的 GitHub / 提交 PR 收起；
装机向导加「让 AI 来安装」勾选，勾了直接把脱敏任务交给维护助手。

## 用户原话落点（别再改回去）

1. **工作台要原来的样子**：对话工作台以 Claude Code 为底座，旁边有终端工作台；「很多用户还是用原来的样子」。
2. **维护助手只用于维护**：装机/故障搞不定时跳出来兜底，「每次都是一个新的对话框」，不要累积历史。
3. **我的 AI 要做精**：只留 Codex / Codex 桌面版 / ClawX / pi / Hermes / DSH / OpenCode / WorkBuddy / UU 远程 / Claude CLI，**magpie 放回我的 AI**；其余都不要。
4. **Opencodex 不要**；工作台精简，之前删掉的冗余功能不要再回来。
5. **GitHub / 提交 PR 那块东西太多**，隐藏掉。

## 已完成（工作树，未提交）

| 文件 | 改动 |
| --- | --- |
| `src/App.tsx` | 恢复工作台挂载（UWorkspace，唯一一处，`paneMode="chat"`）+ AI 创作独立页（Create 懒加载 + `createInitialSub`）;默认页改回 `workbench`；`LEGACY_TAB_FALLBACK`：chat/termwb→workbench、draw/video/qrmerge→create、advanced/usbgenie→toolhub；维护助手加 `key={maint-N}`（**每进一次=新对话框**）；`selfHeightTab` 加 workbench/create |
| `src/components/Sidebar.tsx` | CORE 恢复「工作台 / AI 创作 / 我的 AI / AI 设置」；维护助手挪到「更多」；页脚 GitHub / 提交 PR 两条注释掉（`contributionLinks=[]`，恢复方法见注释）；`NAV_ALIAS` 与 `TabId` 同步 |
| `src/opencodex/UWorkspace.tsx` | 回到原样但做减法：去掉 PassportBoard 与 `view` 状态（护照面板/入口不再出现）、保留待投递 prompt 机制；文件头注释写明口径 |
| `src/opencodex/SessionList.tsx` | 未改内容（已回滚到 HEAD 原版）；不传 `onView` 后护照入口自动不渲染 |
| `src/MaintenanceAssistant.tsx` | 回到 HEAD 版本（自带 `maint-<时间戳>` 新会话、无历史持久化）——正是「每次一个新对话框」的底座 |
| `src/Wizard.tsx` | ① 头部加「让 AI 来安装」勾选；勾选后 `installQueue()` 直接把队列压成脱敏任务包交给维护助手（不跑自动安装）；② `TOOL_NAMES` 收敛到 11 项（删掉已下架工具，免得向导里还有装不上的入口） |
| `src-tauri/src/tools.rs` | `TOOL_SPECS` 24 → **11**：claude-code / codex / codex-app / clawx / hermes / dsh / pi / opencode / magpie / uu-remote / workbuddy；`probe_tool`、`list_tools`、openclaw 判定与单测同步；删除 cline 健康探测死分支 |
| `src/opencodex/apps.ts` | `TUI_APPS` 收到 6 个终端入口（claude/codex/hermes/dsh/opencode/pi），`TuiAppId` 收窄 |
| `src-tauri/skills/install-windows.json`、`website/skills/install-windows.json` | 同步删 9 个工具条目，version 65→66，两份逐字节一致 |
| `src-tauri/src/installer.rs` / `providers.rs` / `lib.rs` | 清单收缩的连带修复：GUARDED_CLIS 与 shim 名单同步；`APPLY_ALL_TARGETS` 摘 qwen/crush 并新增 `LEGACY_RETIRED_TARGETS` 兜住存量机器换 Key；相关单测同步 |
| `src/generated/*` | `action-parity:generate` 重跑（62 个动作），非手改 |
| `src/i18n/en/onboarding.ts`、`maintenance.ts`、`sidebar.ts` | 新增「让 AI 来安装」等英文词条 |

## 验证结果（真跑）

- `pnpm build` → **exit 0**（模块耦合 / 主题 / i18n / skill 清单 / 模型目录 / 供应商模板 / 工具注册表 / tsc / vite / webkit 兼容全过）
- `node scripts/check-tool-registry-sync.mjs` → **exit 0**（TOOL_SPECS 11 / TUI_APPS 6，id·cmd·config_target 一致）
- `node scripts/check-skill-manifest-sync.mjs` → **exit 0**（内嵌 v66 / 热下发 v66）
- `node scripts/check-leak.mjs .` → **PASSED 0 hits**
- `cd src-tauri && cargo check` → **exit 0**（18 条 warning，与改动前同数）
- `cargo test --lib` → 601/602；唯一失败 `agent::chat::tests::curl_config_streams_sse_and_sends_body_over_loopback`（并行偶发 RST，**单跑与 `--test-threads=1` 全绿 602/0**）
- `pnpm tauri build` → **exit 0**，产物 `src-tauri/target/release/u-king-mini.exe`；启动后主窗口正常出现（标题来自前端 `U-King AI 管家`，WebView2 子进程 1 个）

## 还没做 / 待拍板

| # | 事项 | 说明 |
| --- | --- | --- |
| T1 | **Chat 右侧面板**（文件树 / 结果预览 / 浏览器）仍在 | 本轮只摘了「护照」；这几个是 10-20 判过的冗余项，要不要一并删需要用户点头（删掉后右侧只剩终端，产出文件改走系统默认程序打开） |
| T2 | **SessionList 的 worktree 功能**还在 | 属「精简」范畴但不在用户点名里，未动 |
| T3 | **OpenCodex 随包**：工具目录已摘，`resources/OpenCodex.exe` 仍 `include_bytes!` 打进 exe | 只影响体积，不动功能；要瘦身需单独改 `opencodex_bundle.rs` |
| T4 | 真机 UI 验收 | 本机不截屏做 UI 验证（隐私纪律），需在干净机 / 绿色版上看一遍 |
| T5 | `chat.rs` 并行偶发 RST | 那条测试是本分支新增的（密钥走 stdin）。并行跑会偶发 `curl: (56) Connection reset`，串行 602/0 全绿；建议在测试里加 retry 或换独立端口 |
| T6 | 提交 | 全部未 `git add`；提交一律显式 pathspec，别 `git commit -a` |

## 环境坑（本机实测）

- `pnpm build` 的 `vite build` 收尾会被 `esbuild remove …\Temp\esbuild-*: Access is denied` 打断 —— **把 `TEMP`/`TMP` 指到仓库内目录**（例 `.tmp-build`）即可全绿。
- 无头 Chrome/Edge 截图在本机时灵时不灵（默认 profile 被用户正在跑的 Chrome 占用时直接静默退出），要截就配独立 `--user-data-dir` 且输出目录别放 `%TEMP%`。

## 第二轮（同日，用户看过界面后追加）

| 项 | 落点 |
| --- | --- |
| **技术支持搬家** | 侧栏页脚不再放「技术支持」（展开态与收起态两处都删）；改到 **AI 设置页底部**一行：`技术支持 · 报告问题` + `官网 · 充值说明`（`Manager.tsx`，走 `onGoPage`）。页脚现在只剩 语言/主题/检查更新/一键充值/版本号 |
| **magpie 处理** | ① `TOOL_SPECS` 的 `launch_mode` 改为 `GuiApp`（点开直接开窗，和 DSH 一致，不再塞进终端）；② `ToolInfo.launch_app="magpie"`、`hidden:true`（「我的 AI」不再露出、不主推；已装机器仍可检测/启动）；③ 单测 `magpie_gui_shortcut_uses_native_window_entry_and_preserves_cli` 同步断言 `GuiApp` |
| **工作台再减一档** | `Chat.tsx` 右侧入口去掉「文件」（文件树）；保留「终端」「预览」。终端旁那条文件栏暂留（同一份 FilesPanel，彻底删要单独评估） |
| **工具逐个实测** | `runtime.tool.inspect`（只读、不启动）跑全表：**11/11 installed=true、status=ready**。模式：`embedded_pty` = claude-code / codex / hermes / pi / opencode（点开进内嵌终端）；`gui_app` = clawx / dsh / uu-remote / workbuddy / codex-app / magpie（点开直接开窗） |

**待用户拍板的精简候选**（还没动）：
- Chat 右侧「预览」面板（图/网页/文档）—— 删掉后产出文件改走系统默认程序打开；
- 终端旁的文件栏（`termFilesOpen`）；
- `SessionList` 的 worktree（新建工作树）—— 给 git 用户的功能，小白基本用不到；
- 工作台顶部的「大脑」下拉（现在只有 Claude Code 一个选项）。

**第二轮验证结果**：
- `cargo check` exit 0；`cargo test --lib tools::` **22 passed / 0 failed**；`RUST_TEST_THREADS=1 cargo test --lib` **602 passed / 0 failed / 3 ignored**（94.9s，之前那条并行 RST 偶发在串行下不复现）
- `pnpm tauri build` exit 0；新 exe 启动主窗口正常（标题 `U-King AI 管家`）
- 启动实测：`runtime.tool.launch magpie` → `{"message":"已启动 magpie","next":"done"}`，进程有真实窗口句柄（GUI 开窗，不再进终端）；`runtime.tool.launch dsh` → `已启动 DeepSeek Harness（官方桌面版）`
- `runtime.tool.inspect`：11/11 ready；`gui_app` = clawx / dsh / uu-remote / workbuddy / codex-app / magpie，其余 `embedded_pty`

## 第三轮（1.4.2，同日）

| 项 | 落点 |
| --- | --- |
| **维护助手多会话** | 左侧「维护记录」列表 + 「新对话」按钮；消息落 `chat_archive_replace`（`~/.uking/chats/<id>.jsonl`），索引在 localStorage `uking.maint.sessions`；点历史回读（`chat_archive_load`）、可删除（`chat_archive_delete`）；首条用户消息自动当标题；每进一次维护页 = 新开一条（App 侧 key 重挂载） |
| **官方 logo** | `ToolIcon.tsx` + 素材：WorkBuddy 紫色 W 兜底 → 官方 app icon；UU 远程 64px → 官方 256；Hermes 吉祥物头像 → Nous 官方矢量标（`dark:invert`）；DSH → 官方 favicon；magpie → 官方 icon；11 个工具全部有官方素材。来源记进 `NOTICE` |
| **AI 创作 · 在线作图** | 导航新增一条，浏览器打开 https://tu.u-claw.org.cn/ |
| 版本 | 1.4.2（四处同步）+ CHANGELOG；出包并放桌面 `<桌面>\U-King-1.4.2.exe`，启动窗口正常 |

**体积提醒**：`hermes.svg` 91.6KB（gzip 37KB）是新增素材里的大头，整体净增约 +150KB 打包体积；若要保住 exe 体积，可降成 256px PNG（约 8KB）。

**还没做**（等用户拍板）：Chat 右侧预览面板、终端旁文件栏、SessionList worktree、大脑下拉 —— 四个精简候选仍未动。

## 第四轮（1.4.3，同日）

| 项 | 落点 |
| --- | --- |
| **侧栏页脚再减负** | 语言 / 主题 / 检查更新从侧栏搬到 **AI 设置页脚**（和「技术支持 · 报告问题 / 官网 · 充值说明」同一行）；侧栏页脚只剩「一键充值」+ 版本号/官网，矮屏仍留「缩到托盘」 |
| **AI 优化大师精简** | 撤「加杀软白名单·提速」「晒体检分」两个按钮（`addDefender` 保留给使用数据的建议卡；分享卡组件不再在本页出现） |
| **交接提示词修复** | 点「让 AI 给优化建议」等交接入口 → 维护助手**新开一条对话**并**把提示词预填进输入框**。根因：维护助手按「进入即新对话」用 key 重挂载，第一次挂载消费 prompt 后 key 再变 → 第二个实例拿到空 prompt |
| 版本 | 1.4.3，出包放桌面 `<桌面>\U-King-1.4.3.exe`，启动窗口正常 |

**这一轮的功能验证**：`runtime.tool.inspect` 11/11 ready；`magpie` / `dsh` 真启动通过；`pnpm build` 全绿；`pnpm tauri build` exit 0；窗口实测可开。
**没跑成**：`action conformance`（全表跑一遍）超过 10 分钟且会占住 exe 文件、影响出包，已中止；`--selfcheck` 在 GUI 运行时不吐 JSON（环境待查）——两项留作发版前的完整回归。

## 第五轮（1.4.4，同日）

用户拍板「可以删」，四个候选 + 两个按钮全部落地：

| 项 | 结果 |
| --- | --- |
| Chat 右侧「预览」面板 | 删除。`showPreview`/`previewFile`/`Preview` 类型/放大缩小/redline 预览宿主一并移除，产出文件改 `open_produced_file`（系统默认程序） |
| 终端旁文件栏 | 删除（`termFilesOpen` / `termFilePath` / 拖宽 / FilesPanel 引用全清） |
| 右侧「文件」面板 | 删除（入口 1.4.1 已撤，本轮连面板代码一起删） |
| 工作台顶部「大脑」下拉 | 删除（只剩 Claude Code，没有可选项） |
| 侧栏「一键充值」按钮 | 删除（充值走「AI 设置 → 账号 · 充值」）；页脚现在只剩版本号/官网 |
| 维护助手页头余额/充值按钮 | 删除（余额门禁仍在发送前判定，用 toast 提示） |
| 会话列表 git 工作树 | 删除（`SessionList` −154 行、`store.addWorktree` −41 行、`Task.worktree_*` 类型 −3 行）；顺带发现后端从未有 `git_create_worktree` / `git_is_repo` 命令 |

**验证**：`tsc --noEmit` 0 错误；`pnpm build` 8 道静态自检全绿；`pnpm tauri build` exit 0；1.4.4 exe 从 14.30MB 降到 **13.65MB**（删面板/预览内核顺带瘦身约 650KB）；桌面启动窗口正常。

**剩下没动的**：提交（全部改动仍未 git add）。四个精简候选已清零。

## 第六轮（1.4.5，同日）

| 项 | 落点 |
| --- | --- |
| **海外模型默认不列** | `src-tauri/models/xiapan-models.json` + `website/skills/xiapan-models.json` 删掉 `海外` 组（Claude Sonnet/Opus 5.5、GPT-6 Astra、GPT-6.1 Sol），两份 version 2→3；`check-model-catalog-sync` 绿。默认只剩 DeepSeek V4 Flash / V4 Pro / Kimi K3 |
| **设置入口路标** | 语言/主题/检查更新搬进 AI 设置后用户找不到，侧栏页脚版本号旁加「设置」链接（`onSelect("manage")`） |
| **logo 按官方素材重做** | Codex CLI 改 OpenAI 官方 blossom（currentColor）；Codex 桌面版=深色 tile+白 blossom（对照微软商店官方 MSIX unplated logo）；Hermes=Nous 官方 mascot app icon（去外框徽章）；ClawX=官方仓纯蓝龙虾；codex/openai.svg 改 currentColor（浅色不再空白）；WorkBuddy/UU/pi/OpenCode/DSH 逐个核过官方来源，明细在 `NOTICE` |
| 版本 | 1.4.5，桌面 `<桌面>\U-King-1.4.5.exe`，启动正常 |

**口径备注**：UU 远程官方没有公开矢量（最高清是 App Store 官方 app icon 512）；Codex 桌面版 Windows 官方包当前显示的就是 OpenAI/ChatGPT 的 blossom（OpenAI 上游行为）；Hermes 官方没有「无外框无文字」的简洁 mark，只能用 mascot app icon。

## 第七轮（1.4.6，同日）

| 项 | 落点 |
| --- | --- |
| **AI 设置残留工具** | 「AI 设置 → 工具」页的 `TOOL_TABS` 与 `TARGET_TOOL_ID` / `TOOL_LABELS` 里还硬编码着 Qwen Code、Crush（早随工具目录下架），显示成「未接管」卡片；两个 Tab + target 映射 + 标签全部删掉 |
| **侧栏「设置」点击无反应** | 齿轮现在会切到 AI 设置**并滚到页底的「外观/检查更新/技术支持」段**（`#uking-appearance`）；之前的无反应是因为当页就是设置页、没有滚动反馈 |
| **升级失败兜底** | 「我的 AI」工具卡新增「升级失败？让 AI 修」：生成脱敏维护任务交给维护助手（新开对话 + 预填提示词），与装机失败兜底同一条路 |
| 版本 | 1.4.6，桌面 `<桌面>\U-King-1.4.6.exe`，启动正常 |

## 第八轮（1.4.7，同日）—— 全量自检 + 一个真 bug

**测试面（全部真跑，非声称）**
- `action conformance`（88 项全表）：**71 通过 / 17 跳过（写操作被门禁正确拦住、或需要入参）/ 0 失败**，`ok=true`；4 条 recipe（chat 诊断停摆 / driver 全量切换 / doc 改现有文档 / AI 自发现）全 pass；`version=1.4.7`。
- 23 项只读运行时体检（driver/toolbox/stack/readiness/optimizer/footprint/instance/chat/crash/usage/uu/clawx/command_guard/ai_process/workbench/skillpack/identity/origin/journal/hardware/localllm/codex/network）全部返回合法 JSON。
- 11 个工具逐个 `runtime.tool.launch`：5 个 CLI 返回 `embedded_pty` 指令；6 个 GUI（ClawX / DSH / UU 远程 / WorkBuddy / Codex 桌面版 / magpie）**真启动成功**并返回 `done`。
- 模型目录：只剩 3 个国产模型（deepseek-v4-flash/pro、kimi-k3），无海外档。

**发现并修好**：`runtime.identity.inspect` 一直 `not_ready` —— 说明书 `~/.uking/llms.txt` 生成过，但**从来没在 Claude / Codex / 根 AGENTS.md 里挂指针**（10-04 删除 identity 页后就没有入口了）。1.4.7 起启动生成说明书后自动 `identity::link_in(home, &[])`：幂等、只增不删、首次留底 `*.uking-bak`。实测新构建启动后 `ready=true`、3/3 linked。

**未修（非产品问题）**：`browser.snapshot` 首轮偶发失败（本机 Chrome 无沙箱环境，DevToolsActivePort 没写出来），复跑即过；`readiness` 的余额读数有一次显示 0（查询抖动，另一次读到 3997 万 token）。

## 第九轮：1.4.7 正式发布（2026-10-21）

- 提交：`d1eeb520a release: 1.4.7 —— 工作台回原样、工具目录收敛到 11 个、界面减法、官方 logo、交接兜底`（54 文件，+1255/−2077），已合入 `main` 并推送；tag `v1.4.7` 已推。
- Mac 包：GitHub Actions `build-mac`（run 39586540453，headSha=d1eeb520a）成功，产物 `U-King-Mac.zip` + `U-King-Mac.dmg` + `mac-release-proof.json`（ad-hoc 签名，version 1.4.7）。
- 发布：`node scripts/release-uking.mjs publish --config shots/release-147-config.json` 成功（中途两次因长途 scp "Connection reset" 失败，给 `~/.ssh/config` 的海外镜像主机段补了 `IPQoS 0x00 / Compression yes / ServerAliveInterval 15` 后一次通过；原配置备份在 `~/.ssh/config.bak-20261021-sg`）。
- 线上核验：
  - `https://u-claw.org.cn/uking/version.json` → **1.4.7**（2026-10-21）
  - `https://u-claw-updates.oss-cn-shenzhen.aliyuncs.com/uking/version.json` → **1.4.7**
  - 下载：`U-King.exe` 13,632,000 B、`U-King-Mac.zip` 10,035,822 B、`U-King-Setup.exe` 5,169,240 B、`U-King-Mac.dmg` 10,719,641 B（与本地产物逐字节一致）
  - 热下发：装机清单 **v66**、模型目录 **v3**（国内与海外镜像均已是新版）
- 发布产物与 manifest 留在 `shots/release-1.4.7/`（gitignore，不入库）。

## 第十轮：1.4.8 发布（2026-10-21 晚）

**用户口径**：官网加 GitHub 入口；「每次对话消耗有点大」要测；技能包做减法。

| 项 | 落点 |
| --- | --- |
| 官网 GitHub | `website/index.html` 页脚新增「GitHub 开源仓库」→ github.com/dongsheng123132/u-king（线上已核验 1 处命中）；已 scp 到国内 + 海外两处站点目录（带 `.uking-backup/site-*` 备份） |
| 官网动态 | `whats-new.json` / `whats-new.html` 置顶 1.4.8 说明；`version.json` 顶部 1.4.8、1.4.7 进 history |
| 技能包减法 | 14 → 8：下线 vision / teamwork / web / mail / browse / pdf（`skillpack.rs` 删 const + PACKS 条目，新增 `RETIRED_PACKS` + `prune_retired()` 在启动同步时精确清理老机器；CAD 保留——`doc.rs` 的 CAD 动作直接调它的脚本）。实测本机清掉 **31 个**已下线包目录（~/.claude/skills 等 4+ 个落点），`~/.claude/skills` 里 6 个包已消失 |
| 对话成本 | `historyTrim` 硬上界 12 万 → **4 万字符**（约 1/3）；`MaintenanceAssistant` 之前**全量重发**历史，现在也走 `trimHistoryForPayload`。技能包少 6 份 SKILL.md ≈ 每轮上下文少 **21,920 字符**（实测 skills/*/SKILL.md 累计 59,182 → 37,262） |
| 工作台模板 | `workbenches/creator/workbench.json` 去掉已下线技能引用（否则校验不过） |
| 发布 | 提交 `640d92a1b`、tag `v1.4.8`；Mac 由 GitHub Actions（run 39605243672）出包；`release-uking.mjs publish` 成功 |

**线上核验**：`u-claw.org.cn/uking/version.json` = **1.4.8**（2026-10-21）；`U-King.exe` 13,534,720 B（比 1.4.7 小 ~97KB，删掉的技能包素材）；`index.html` GitHub 链接命中；`whats-new.json` 首条 = 1.4.8；装机清单 v66 / 模型目录 v3 保持。

## 第十一轮：1.4.9 发布（2026-10-21 深夜）

**用户口径**：「看图 DeepSeek 默认都能看了」+「不需要那些乱七八糟的技能，都删了，会怎么样，其他你自己看着办」。

| 项 | 落点 |
| --- | --- |
| AI 目录只剩 3 个默认包 | 新增 `prune_non_default()`：启动同步把 `pack_names()` 里**非默认、非客户手装**的包从各 AI 工具目录精确清掉。默认 3 个 = aigc / office-read / workbench |
| 手装包不被误删 | 新增 `~/.uking/skills-user.json` 记录「技能包」页手动装的包；`install_pack` 写入、`uninstall_pack` 移除、清理时跳过 |
| 其余技能不占上下文 | ppt / docx / xlsx / cad / office-edit 仍编进安装包、仍导出到 `~/.uking/skills` —— 软件自己的「出 PPT/Word/Excel/CAD」动作直接调脚本，不经过 AI 上下文、不花 token |
| 实测清理 | 本机启动后日志：`AI 目录只留默认包，清掉 25 个非默认包目录`；`~/.claude/skills` 里 U-King 包只剩 3 个，客户自己装的巡检 / 验证类包（`uking-verify` 等）一个没动 |
| 上下文体积 | AI 每轮要读的技能说明：14 包时代 58,712 字符 → 1.4.8 的 8 包 36,939 → **现在 3 包 25,484**（较最初 -57%）；叠加 1.4.8 的历史裁剪（12 万→4 万字符） |
| 发布 | 提交 `b41923b82`、tag `v1.4.9`；Mac CI run 39621368393；`release-uking.mjs publish` 成功 |

**线上核验**：`version.json` = **1.4.9**；`U-King.exe` 13,536,768 B；`whats-new.json` 首条 = 1.4.9；装机清单 v66 / 模型目录 v3 不变。
