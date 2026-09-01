# AGENTS.md

给 AI 编码助手的仓库说明。本目录是 **U-King 的公开开发仓**。

## 这是什么

U-King：面向小白用户的「AI 装机管家 + 多 AI 工作台」桌面应用（Tauri 2 + React 18 + TypeScript + Rust）。绿色版 exe 约 4.4MB，U 盘即插即用，Windows/Mac。

- **对话式装机向导**（`src/Wizard.tsx` + `src-tauri/src/installer.rs`）：体检 → 装 Node/Codex CLI/OpenClaw → 选驱动 → 连通实测；安装逻辑是 JSON skill 清单（内嵌兜底 + 官网热下发覆盖），缺 Node 自动装便携版并写 PATH，装完 verify_cmd 验证、失败自动修复重试
- **模型供应商切换**（`src-tauri/src/providers.rs`）：cc-switch 式；写 `~/.claude` / `~/.codex` 等配置文件，连通性实测（真实调模型）；用户可自带 key，也可选官方中转（计费在服务端）
- **应用内真终端**（`src-tauri/src/term.rs` + xterm.js）：Rust 起 PTY（portable_pty），注入 PATH 与工具 env；reader/writer 热路径零 unwrap（release 是 panic=abort）
- **多 AI 工作台**（`src/opencodex/`）：会话/任务/文件面板，按项目组织，PTY 常驻保活
- **设备 Key**（`src-tauri/src/device.rs`）：机器指纹单向哈希生成 `sk-` key，选官方中转时用于计费；不用该服务则无任何相关请求

## 仓库约定

- `src/generated/` 是 action-parity 产物，**禁止手改**；业务动作改 `src-tauri/src/actions.rs` / `lib.rs`，完成前跑 `pnpm run action-parity:verify`
- 版本号四处同步（发版必同改）：`src/version.ts` / `package.json` / `src-tauri/tauri.conf.json` / `src-tauri/Cargo.toml`
- 热路径（终端读写、上报）不加 unwrap/panic；HTTP 一律走系统 curl.exe + `CREATE_NO_WINDOW`
- 纯 std 优先，加依赖要走体积评估（exe 体积是硬约束）

## 常用命令

```bash
pnpm install          # 前端依赖
pnpm build            # 前端构建 + 全部静态自检（模块耦合/i18n/主题/skill 清单同步）
pnpm tauri dev        # 开发模式
cd src-tauri && cargo check   # Rust 快速类型检查
node scripts/check-leak.mjs . # 泄漏闸门（push 前必跑，CI 也会跑）
node scripts/check-term-file-links.mjs  # 终端链接跑道（[2/2] 需 pnpm dev + Tauri webview 环境）
```

## 敏感信息纪律（本仓是公开仓）

- 提交前跑 `node scripts/check-leak.mjs .`；CI 强制。规则有误报时改闸门白名单要**保守**（白名单条目必须带注释说明理由）
- 注释里禁止出现：真实客户姓名/主机名/客户机编号、服务器 IP 与部署路径、内部仓名、供应商真实代号（用「上游 A」「工单 #N」这类匿名指代）
- 测试夹具一律用一眼假的占位（`demo` / `user1` / `example.com` / `sk-abc123`）
- 客户数据、bug 原始日志、运营文档永远不进本仓（维护者在私有 ops 仓管理）

## 历史说明

本仓自 2026-09-01 起为唯一公开开发仓（首发 v1.2.1）。此前为私有开发，git 历史未迁移（含发布二进制与内部文档）。更早的排障叙事在代码注释里以「工单 #N」「客户机实锤」等匿名形式保留。
