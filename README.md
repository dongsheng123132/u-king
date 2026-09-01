U-King —— 让不会配环境的用户，10 分钟用上 Codex CLI / OpenClaw 等主流 AI 工具。
*AI installer and multi-AI workspace for people who don't want to touch a terminal.*

[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE) [![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS-lightgrey.svg)](https://github.com/dongsheng123132/u-king/releases) [![Release](https://img.shields.io/github/v/release/dongsheng123132/u-king)](https://github.com/dongsheng123132/u-king/releases/latest)

## 这是什么

U-King 是一个桌面应用，把「体检电脑 → 装 Node / Codex CLI / OpenClaw → 配置模型供应商 → 跑通对话」这套流程做成对话式向导。给的是不想碰终端的用户，装完之后应用内自带一个真终端，想手动操作也行。

技术栈 Tauri 2 + React 18 + TypeScript + Rust，绿色版是单个 exe，U 盘插上就能跑。

## 30 秒上手

从 [Releases](https://github.com/dongsheng123132/u-king/releases/latest) 或官网 [u-king.org](https://www.u-king.org) 下载：

- Windows 绿色版 exe 约 11MB，安装版约 5MB
- macOS 提供 dmg / zip

双击绿色版即可运行，无需安装、无需管理员权限。

源码构建：

```bash
pnpm install
pnpm tauri build
```

只想验证代码能不能编译、不出安装包：

```bash
cd src-tauri && cargo check
```

## 核心特性

- **对话式装机向导**——体检环境 → 装 Node / Codex CLI / OpenClaw → 驱动切换 → 连通实测，全程有反馈，失败自动修复重试
- **cc-switch 式模型供应商切换**——一键写入 `~/.claude`、`~/.codex` 等配置文件，本地完成，不用手改 JSON；连通性做真实调用测试
- **应用内真终端**——基于 PTY（portable_pty），自动注入 PATH 与工具 env，装完直接能用
- **多 AI 工作台**——会话、任务、文件面板按项目组织统一管理，PTY 常驻保活
- **单文件绿色版**——Windows / Mac 通用，U 盘携带，无需安装权限

## 功能地图

- `Wizard` 装机向导 —— 体检 / 安装 / 驱动配置 / 连通实测的主流程
- `Manager` / `AiRuntime` —— 已装工具与模型驱动的管理面板
- `opencodex/` —— 多 AI 工作台：会话、任务、文件三栏
- `CodexZone` / `LocalLLM` —— Codex CLI 与本地模型专项面板
- `Backup` / `Advanced` / `Feedback` —— 备份还原、进阶设置、反馈与远程协助入口
- 其余如 `Draw`、`Geo`、`QrMerge` 等是内置的小工具面板，装机之外的日常辅助功能

## 隐私与安全

- API key 只存在本地配置文件（`~/.claude`、`~/.codex` 等），不加密、不上传、不经过 U-King 的服务器
- 默认只在安装失败 / 程序崩溃时上报技术日志（脱敏，不含 key）；截图类反馈需用户手动触发才会发送
- 代码开源可审计，装机与配置逻辑都在本仓库里

远程协助默认关闭，需要用户在反馈页主动开启，可随时停止，2 小时自动断开，每条命令记本机审计日志。完整条款见 [PRIVACY.md](PRIVACY.md)。

发布的 exe 附带 SHA-256 校验值，下载后可自行核对，见 [SECURITY.md](SECURITY.md)。安全漏洞请走 SECURITY.md 里的私密渠道，不要开公开 issue。

## 开发

技术栈：Tauri 2 + React 18 + TypeScript + Rust，优先 std、PTY 用 `portable_pty`，加依赖要过体积评估（exe 体积是硬约束）。

目录结构：

```
src/         前端界面（React + TS）
src-tauri/   Rust 后端：装机、供应商切换、终端、设备等动作实现
```

验证三件套：

```bash
pnpm install
pnpm build              # 前端构建 + 全部静态自检
cd src-tauri && cargo check
```

业务动作只实现一次：每个装机 / 配置 / 终端等动作在 `src-tauri/src/actions.rs` 里只写一份核心逻辑，GUI、CLI（`--selfcheck` 等）都是它的调用方，不是第二份实现。`src/generated/` 是自动产物，不手改。

## 反馈与贡献

报 bug 用 [GitHub issue 模板](https://github.com/dongsheng123132/u-king/issues/new/choose)，自带 `--selfcheck` 输出引导，诊断信息比截图有用。安全漏洞走 [SECURITY.md](SECURITY.md) 私密渠道，不要走公开 issue。

这是单人维护的仓库，PR 会看但不保证时间。欢迎 bug 修复、文档改进、供应商模板、i18n 翻译；新增功能模块 / 重构 / 引入新依赖请先开 issue 商量。详见 [CONTRIBUTING.md](CONTRIBUTING.md)。

## 商业与开源

软件本体永久免费开源。官方提供可选的 API 中转服务（虾盘云），按 token 计费；也可以完全自带 key，供应商切换全部在本地完成，不依赖该服务。

## License

Apache-2.0，见 [LICENSE](LICENSE)。授权不包含 U-King 名称、logo 及虾盘云相关商标 / 域名的使用权，详见 [NOTICE](NOTICE)。
