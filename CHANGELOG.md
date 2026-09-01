# 更新日志

用户可见的版本说明在应用内点左下角版本号查看。本文件是**面向开发者**的提交级历史。

> 版本号四处同步：`src/version.ts` / `package.json` / `src-tauri/tauri.conf.json` / `src-tauri/Cargo.toml`，
> 外加官网下发的更新说明（自升级判据）。

## 1.2.1（2026-09-01）

**Initial public release（首次公开发布）**

- 本仓库自 2026-09-01 起为唯一公开开发仓；此前为私有开发（历史含发布二进制与内部文档，未迁移）
- 功能与私有期最后一个内部版本一致：对话式装机向导、模型供应商切换、应用内终端、多 AI 工作台
- 开源前完成了全树敏感信息审计（客户标识、内网拓扑、供应链命名已脱敏；细节见 `scripts/check-leak.mjs` 闸门）

## 0.9.76（2026-07-28）

**用户可感知**
- 修复「出图失败：请求太频繁」——**根本不是你点太快**。gpt-image-2 的海外上游服务出口 IP 被
  Cloudflare 限速（error 1015），全量 429，等多久都不会好。已把 gpt-image-2 的上游路由切回备用线路（服务端改动，无需升级即生效）
- 作图恢复模型可选：新增「通义千问图片 · 阿里」（国内直连，最稳）和「Seedream 4.0 · 字节」。
  以前只有一个模型且下拉被藏起来，默认模型一挂客户**在界面上找不到任何逃生口**
- 某个模型的上游挂了时，后端自动换阿里直连重画一次，客户零操作（出图后如实告知换了谁）
- 出图失败的建议改成按「哪种失败 + 哪个模型」给。旧文案对所有失败都回一句「建议改用 GPT Image 2」——
  而客户正用着它，等于把人劝进死胡同
- 「意见反馈」页新增**远程协助**：一键开启后显示协助编号发给作者即可远程排查，
  可随时停止、2 小时自动断开、每条远程命令都记进本机审计日志（默认关闭，需主动开启）

**开发者**
- `providers.rs`：新增 `is_upstream_outage` + 两个分场景兜底常量。**安全拒绝**兜到 seedream（审核宽松），
  **上游故障**兜到 qwen（供应商独立）—— 查路由表发现 seedream 和 gpt-image-2 走同一家上游，
  兜过去等于原地踏步
- 新增 `remote_assist.rs`（独立可插拔）+ 无头自检 `--assist-test [保持秒数]`。
  2 小时超时原本在 `agent.ps1` 外壳里，直接起进程会绕过，故模块内自带看门狗；
  停止只按 PID + 校验路径，绝不 `taskkill /IM agent.exe`
- 服务端图片代理模块：429 分「上游限流」和「我方限流」两种归属。
  旧代码一律归成 `Severity:"user"` + 「避免连续快速点击」，把全量事故标成用户问题
- `models.ts`：加模型前必须逐档跑 `IMAGE_SIZES`（wanx2.1-t2i-turbo 非方图必炸，故意不收）

## 0.9.70（2026-07-26）

**用户可感知**
- 修复 Token 压缩机开了也不生效：hook 改写出的裸 `rtk` 不在 PATH 上（`fix(rtk)`）
- 「安全卸载」足迹扫描 release 6.2s → 2.6s，输出逐字节一致（`perf(cleanup)`）
- 新增「更新日志」弹层：点侧栏版本号查看（服务端 `version.json` 的 notes/history 以前从没被渲染过）
- 能力卡改报「能不能用」而非「装没装」：rtk / ollama / geo 带 `ready` + `blockers`（`feat(actions)` readiness）

**内部**
- 影核协议改造：33 个动作（17 只读 + 16 写），27 个老 command 变薄壳，GUI / CLI / MCP 三面共用一份核心
- 写动作核心强制确认 + `expected_state_version` 乐观并发；`destructive` 类单独标注
- `action conformance` 通用回归跑道（33/33）+ `action bindings` 绑定核对 + `mcp serve`
- 本地用量统计流式读 + 预筛，release 快 35%（`perf(usage_local)`）
- 小程序运行时内核（清单 / 安装 / 无头执行 / 权限 / 产出箱 / 图像原语），**暂无 UI 入口，对用户不可见**

## 更早

```
2026-07-26|fix(miniapp): 认一组规范版本，别死钉一个 + warpPerspective 原语
2026-07-26|feat(actions): readiness 约定 —— 动作回答「能不能用」，不是「装没装」
2026-07-26|security(miniapp): 无头执行上 Node 权限模型 —— 「密钥不下发」原本是假的
2026-07-26|fix(rtk): Token 压缩机开了也不生效 —— hook 改写出的裸 `rtk` 不在 PATH 上
2026-07-26|feat(miniapp): uking.image.* 宿主图像原语 + 产出箱接线
2026-07-26|feat(mcp): mcp serve —— 影核第三个面，AI 直接驱动 U-King
2026-07-26|feat(actions): GUI 控件挂 data-action-id + 绑定核对（宪法 14/15 收口）
2026-07-26|feat(actions): 影核协议波次 3 —— 进度协议 + 最危险的 5 个长任务进核心
2026-07-26|feat(actions): 写动作门禁补全 —— 11/11 全部进核心，新增 destructive 类
2026-07-26|feat(actions): 影核协议第三波 —— 写动作 + 核心强制的确认与乐观并发
2026-07-26|perf(cleanup): 足迹扫描改并行探测（release 6.2s → 2.6s，结果逐字节一致）
2026-07-26|perf(usage_local): 流式读 + 便宜预筛（release 快 35%，结果逐字节一致）
2026-07-26|refactor(actions): 影核协议第二波 —— 只读诊断迁完 + 入参真的会校验
2026-07-25|refactor(actions): 影核协议第一波 —— 动作核心去业务化 + 通用回归跑道
2026-07-25|feat(miniapp): 产出箱 artifacts.rs —— 无头调用交引用不交像素
2026-07-25|chore: 版本推到 0.9.70（小程序功能所在版本）
2026-07-25|feat(miniapp): 小程序运行时内核 —— 清单/安装/无头执行/权限
2026-07-25|feat(miniapp): uking:// 协议 spike + 小程序 IPC 门禁
2026-07-25|release: 0.9.69 Mac universal 产物（CI run 30160203783）
2026-07-25|release: 0.9.69 version.json（运行时自愈 + 意见反馈增强 + backup 修复 + 反馈UI）
2026-07-25|release: 0.9.69 版本号四处对齐（+ Cargo.lock）
2026-07-25|polish(sidebar): 意见反馈入口归入页脚工具组，不再突兀
2026-07-25|feat: 意见反馈诊断增强 —— 粘贴截图 + 自动带装机日志 + AI 崩溃取证
2026-07-25|fix(backup): 别再按裸镜像名杀 Claude——还原/测试会端掉用户正在跑的 AI 会话
2026-07-25|fix(installer): 便携 Node 也上自愈——补齐「一套确定的运行时」
2026-07-25|fix(installer): 便携 Python 自愈——根治 ~17 个客户机装机 bug
2026-07-25|release: 0.9.68 Mac universal 产物（CI run 30149037563）
2026-07-25|release: 0.9.68 version.json (命令守卫/优先级修复 + 意见反馈 + 崩溃修复 + 安全卸载)
2026-07-25|chore: bump Cargo.lock to 0.9.68
2026-07-25|feat+fix: 0.9.68 —— 影核协议命令守卫 + 意见反馈 + 演示卸载绿色版 + 安全卸载逐项清理；修 #191 崩溃
2026-07-24|feat(uuswitch): 一键导入改「直接写 SQLite 库」—— 老用户也可靠生效，只增不覆盖
2026-07-24|feat(providers): 驱动回显以活配置为准（claude/codex）—— 和 uu-switch/外部改动同步
2026-07-24|release: 0.9.67 Mac universal 产物（CI run 30060794285）
2026-07-24|release: U-King 0.9.67 —— 修影爆「技能脚本缺失」(aigc 脚本内嵌 exe 自释放)
2026-07-24|fix(yingbao): T-King「技能脚本缺失」修复 —— 内嵌 aigc 脚本 + 运行时释放兜底
2026-07-23|release: 0.9.66 Mac universal 产物（CI run 30020024522）
2026-07-23|release: U-King 0.9.66 —— uu-switch 一键导入(虾盘云 Claude+Codex + 在用配置) + 影爆修可用模型卡死
2026-07-23|fix(yingbao): T-King「可用模型」永不卡死 —— list-models 加 20s 超时 + 前端重试
2026-07-23|feat(uuswitch): 导入扩到 Codex + 在用配置 + 摸清 cc-switch v3.18 DB 存储模型
2026-07-23|feat(uuswitch): 一键把虾盘云导入 uu-switch —— U-King / uu-switch 切换等效
```
