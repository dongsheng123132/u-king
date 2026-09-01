# U-King

> 把 AI 开发环境装好、配好、跑通；不想碰终端也能开始用。

*An open-source AI installer and workspace for Windows and macOS.*

[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
[![Release](https://img.shields.io/github/v/release/dongsheng123132/u-king?display_name=tag)](https://github.com/dongsheng123132/u-king/releases/latest)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS-4c6ef5.svg)](https://github.com/dongsheng123132/u-king/releases)

[下载最新版本](https://github.com/dongsheng123132/u-king/releases/latest) · [官网](https://www.u-king.org) · [提交问题](https://github.com/dongsheng123132/u-king/issues/new/choose) · [参与贡献](CONTRIBUTING.md)

## 它解决什么

AI CLI 的第一步常常卡在 Node、PATH、镜像、配置文件和模型连通性。U-King 把这条路做成一个桌面向导：先体检，再安装、配置并做真实连通测试；之后仍可在应用内终端和工作台里继续使用。

```text
体检电脑  →  安装运行时与 AI 工具  →  配置模型供应商  →  连通实测  →  开始工作
```

适合刚接触 AI 编程工具、不想手改环境配置的用户；也适合需要把常用工具放在 U 盘中携带的场景。

## 30 秒开始

从 [GitHub Releases](https://github.com/dongsheng123132/u-king/releases/latest) 或 [官网](https://www.u-king.org) 下载对应平台的版本。

- Windows：绿色版双击即用，无需管理员权限；也提供安装版。
- macOS：提供 dmg / zip。
- 发布页附有 SHA-256；下载后可按 [安全校验说明](SECURITY.md#构建产物校验) 核对。

打开 U-King 后，按向导选择要安装的工具和模型供应商即可。已有 Node、CLI 或配置不会被静默删除；安装失败会给出过程反馈并尝试修复。

## 能做什么

| 场景 | U-King 提供的能力 |
| --- | --- |
| 安装 AI 工具 | 自动准备 Node 运行时与 PATH，并安装、验证和修复常用 CLI。当前清单覆盖 Claude Code、Codex CLI、Pi、OpenClaw、OpenCode、Qwen Code、Crush、Cline 等。 |
| 配置模型 | 以本地配置文件为中心，写入 Claude Code、Codex 等工具的供应商设置；支持自带 key，也可选择官方可选中转。 |
| 验证是否真的可用 | 不只判断“装好了”，还会执行版本检查与模型连通测试，失败时提供可见的诊断过程。 |
| 日常使用 | 内置基于 PTY 的真终端，自动带上工具环境；多 AI 工作台按项目组织会话、任务和文件。 |
| 便携与维护 | Windows 绿色版可随 U 盘携带；提供备份还原、进阶设置、反馈与按需开启的远程协助入口。 |

工具清单与安装逻辑是可审计的 JSON skill；新工具可随清单更新，不必等待桌面端重新发版。

## 隐私与安全

- API key 留在本机的工具配置目录（如 `~/.claude`、`~/.codex`），不会被 U-King 上传或经过其服务器。
- 默认仅在安装失败或程序崩溃时上报脱敏技术日志；截图反馈须由用户手动发起。
- 官方 API 中转是可选服务，用户可以完全使用自己的 key，不依赖它。
- 远程协助默认关闭，须用户主动开启；可随时停止、2 小时自动断开，并在本机记录审计日志。

完整说明见 [PRIVACY.md](PRIVACY.md)。发现安全漏洞请不要开公开 issue，改走 [私密披露渠道](SECURITY.md)。

## 从源码构建

前端为 React 19 + TypeScript，桌面壳为 Tauri 2，后端使用 Rust。需先安装 Node.js、pnpm 和 Rust 工具链。

```bash
pnpm install
pnpm tauri build
```

日常开发可用：

```bash
pnpm tauri dev
```

提交改动前，请在仓库根目录依次通过公开仓的三道闸门：

```bash
node scripts/check-leak.mjs .
cd src-tauri && cargo test --lib && cd ..
pnpm build
```

`src/generated/` 是自动生成文件，不能手改。业务动作只在 Rust 动作核心实现一次，GUI、CLI 和测试只调用它；相关改动完成后另跑：

```bash
pnpm run action-parity:verify
```

## 参与贡献

欢迎 bug 修复、文档改进、供应商模板、i18n 翻译，以及对已支持工具（包括 Pi）的兼容性反馈。新增功能模块、较大重构或新增依赖，请先开 issue 讨论边界和方案。

本项目由个人维护，PR 会认真阅读，但不承诺即时合并。具体约定见 [CONTRIBUTING.md](CONTRIBUTING.md)。

## 开源与商标

软件本体以 [Apache-2.0](LICENSE) 开源。官方提供可选的虾盘云 API 中转服务，按 token 计费；使用它不是运行 U-King 的前提。

Apache-2.0 授权的是源代码，不授予 “U-King”、logo、“虾盘云”及相关域名的商标使用权；详见 [NOTICE](NOTICE)。
