U-King —— 让不会配环境的用户，10 分钟用上 Codex CLI / OpenClaw 等主流 AI 工具。
*AI installer and multi-AI workspace for people who don't want to touch a terminal.*

[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE) [![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS-lightgrey.svg)](https://github.com/dongsheng123132/u-king/releases)

## 这是什么

U-King 是一个桌面应用，把"体检电脑 → 装 Node/Codex CLI/OpenClaw → 配置模型供应商 → 跑通对话"这套流程做成对话式向导。绿色版 exe 约 4.4MB，U 盘即插即用，不需要用户自己排查环境问题。

## 30 秒上手

从 [Releases](https://github.com/dongsheng123132/u-king/releases) 或官网 [u-king.org](https://www.u-king.org) 下载对应平台的绿色版 exe，双击运行即可，无需安装。

源码构建：

```bash
pnpm install
pnpm tauri build
```

快速验证（不出安装包）：`cd src-tauri && cargo check`

## 核心特性

- 对话式装机向导：体检环境 → 装 Node/Codex CLI/OpenClaw → 驱动切换 → 连通实测，全程有反馈
- cc-switch 式模型供应商切换：一键写入 Claude Code / Codex 配置，本地完成，无需手改 JSON
- 应用内真终端：基于 PTY，自动注入 PATH，装完即可直接用
- OpenCodex 式多 AI 工作台：会话、任务、文件面板统一管理多个 AI 工具
- 单文件绿色版：Windows/Mac 通用，U 盘携带，无需安装权限

## 隐私与安全

- API key 只存在本地配置文件（`~/.claude`、`~/.codex` 等），不加密上传，不经 U-King 的服务器
- 默认只上报安装失败 / 崩溃时的技术日志，不含 key；截图类反馈需用户手动触发
- 代码开源可审计，装机与配置逻辑均可在本仓库查看

详见 [PRIVACY.md](PRIVACY.md)。

## 商业与开源

软件本体永久免费开源（Apache-2.0）。官方提供可选的 API 中转服务（虾盘云）按 token 充值，用户也可完全自带 key，供应商切换全部在本地完成，不依赖该服务。

## 开发

技术栈：Tauri 2 + React 18 + TypeScript + Rust（优先 std，PTY 用 portable_pty）。构建产物无外部服务端依赖。

目录：`src/` 前端界面，`src-tauri/` Rust 后端与动作实现。

## License

Apache-2.0，见 [LICENSE](LICENSE)。本授权不包含 U-King 名称、logo 及虾盘云相关商标 / 域名的使用权，详见 [NOTICE](NOTICE)。
