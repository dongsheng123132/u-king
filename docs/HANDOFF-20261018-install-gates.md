# 任务交接 · 装机验收门禁 + TEMP 根因（2026-10-18）

> 交接给 Claude。本文只写「做到哪、剩下什么、别踩什么」，细节在 `docs/install-gate-acceptance.md`。

## 一句话

U-King 装机验收原先只看退出码，Bun 壳 / MiMo SQLite 挂都报「安装成功」。已加两层门禁（输出门禁 + 装后冒烟）拦假通过，并把 TEMP 不可写的根因查清、便携启动器全部修完。

## 已完成（可验证）

### 1. 装机门禁（u-king，已推 `main` `7c44f6bdd..140588190`）

| 提交 | 内容 |
| --- | --- |
| `c99fc95f1` | ToolSpec 加 `verify_expect/reject` + `smoke_cmd/expect/reject`；`output_gate` + `run_smoke`；清单 v64 |
| `10040b46a`–`140588190` | 验收记录 / L2 根因 / L3 遗留项 / 启动器 TEMP |

- **OpenCode**：`verify_cmd` 覆盖 `--help`，钉 `bun is a fast`；`npm_install` 带 `with_optional`
- **MiMo Code**：`smoke_cmd: mimo db "SELECT 1"` 真触 SQLite
- 无头跑道：`--gate-catch-test`（假二进制反向实测）、`--install-gate-test`（真实装机）
- 单测：`cargo test --lib` **631 passed**（含 `embedded_manifest_gates_*`、`output_gate_*`、`verify_rejects_bun_shell_*` 等）

### 2. TEMP 根因（L2，已闭合）

**不是有人恶意改 TEMP**，是便携启动器有意钉到 U 盘。`TEMP` 不可写时 MiMo 一次性 FTS 迁移必挂：

```
SQLiteError: unable to open database file
Failed to run the query 'DROP TABLE IF EXISTS `memory_fts_idx`;'
```

A/B 实测（`shots/cli-startup-20261018/`）：同一 DB home，只换 TEMP 就能翻结果。

| TEMP | 全新库迁移 |
| --- | --- |
| 正常目录 | ✅ |
| `F:\temp`（盘不在） | ❌ 同款报错 |
| 是文件不是目录 | ❌ 同款报错 |

### 3. 便携启动器 TEMP 回退（三仓，已推）

| 仓 | 提交 | 内容 |
| --- | --- | --- |
| u-king | `ac037317b` | `launch-agent.cmd` 写探针回退 + 测试钉死 |
| u-claw (v3) | `8434fbf` `350c6e5` | `Windows-Start.bat` + `Mac-Start.command` |
| u-model | `c9fc0a0`（分支 `codex/action-parity-pilot`） | `启动.*` 三平台 + `portable.py` |

统一做法：`set TEMP|TMPDIR` 到 U 盘后写探针，失败回退系统临时目录。  
`启动.bat` 先留 `HOST_TEMP`——`LOCALAPPDATA` 已被改写，不能拿它当回退。

## 剩下什么（遗留项，见验收文档 L1–L3）

| # | 事项 | 状态 | 阻塞 |
| --- | --- | --- | --- |
| **L1** | OpenCode 装出 Bun 壳 | 搁置 | 上游 `opencode-ai` 包/postinstall，我们改不了 |
| **L2** | MiMo TEMP/SQLite | 根因已定位 | 根治要上游迁移别假设 TEMP 可写 |
| **L3** | TEMP 残留 | 两条解不了 | ① 用户自开外部终端 ② 同 L2 上游 |

## 工作区未提交（不是本次工作，别动）

u-king `git status` 有**并行会话**的改动，与门禁无关：

```
scripts/check-manager-launch.mjs
scripts/check-tool-registry-sync.mjs
src-tauri/src/providers.rs / term.rs / tools.rs
src/Manager.tsx / Wizard.tsx / components/CustomProviderModal.tsx
src/lib/providerPresentation.ts / providerTemplates.ts
src/opencodex/apps.ts
docs/排查-0xc0000142-reg.exe-客户机.md
scripts/check-agent-cli.mjs / verify-provider-endpoints.mjs
```

u-model 有未跟踪 `AGENTS.md`（系统文件，通常不进仓）。

## 复现 / 验收命令

```bash
# u-king 全量
cd src-tauri && cargo test --lib          # 631
node scripts/check-skill-manifest-sync.mjs

# 门禁反向实测（不依赖 npm）
src-tauri/target/debug/u-king-mini.exe --gate-catch-test

# 真实装机（慢，真 npm install）
src-tauri/target/debug/u-king-mini.exe --install-gate-test

# u-model
cd v2/u-model && python -m pytest -q      # 53
```

## 别踩的坑

1. **`launch-agent.cmd` 必须纯 ASCII**（测试 `launcher_is_ascii_and_uses_stable_current_path` 会红）
2. **skill 清单两份必须一致**：`src-tauri/skills/install-windows.json` = `website/skills/install-windows.json`，改一边要 `node scripts/check-skill-manifest-sync.mjs`
3. **TEMP 回退不能用 `LOCALAPPDATA`**——启动器可能已把它改写到 U 盘（见 `启动.bat` 的 `HOST_TEMP`）
4. **MiMo 冒烟走 `tool_process_env`**（TEMP 钉 `~/.uking/tmp/mimo`）——验证的是「绕过后能用」，不是「默认 TEMP 不会挂」
5. **u-king 并行改动别混进提交**；只 add 自己动过的文件

## 主文档

- 验收 / 门禁 / 遗留项：`docs/install-gate-acceptance.md`
- 本交接：`docs/HANDOFF-20261018-install-gates.md`
