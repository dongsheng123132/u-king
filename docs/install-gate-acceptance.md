# 安装验收门禁 · 验收记录

> 本记录只写已经观察到的证据；未列出的项目不是默认通过。
> 背景：U-King 装机验收原先只看 `verify_cmd` 退出码，Bun 壳（`opencode --version` 打 `1.3.14` 退出 0）
> 和 MiMo SQLite 初始化失败（`mimo --version` 是绿的）都会被报成「安装成功」。

## 门禁机制（skill 清单 v64）

`ToolSpec` 新增五字段，不填 = 不检查（老清单热下发安全）：

| 字段 | 作用 |
| --- | --- |
| `verify_expect` | verify_cmd 输出必须包含（不区分大小写） |
| `verify_reject` | verify_cmd 输出禁止包含 |
| `smoke_cmd` | 装后冒烟命令，必须退出 0 |
| `smoke_expect` | 冒烟输出必须包含 |
| `smoke_reject` | 冒烟输出禁止包含 |

流程：`steps` → `verify_cmd`（退出码 + 输出门禁）→ `smoke_cmd`（能力冒烟）→ 失败进 `repair` 再验。

清单钉死：

- **OpenCode**：`verify_cmd` 覆盖 `--help`（`--version` 只吐 `1.3.14` 拦不住）；`verify_reject`/`smoke_reject` 钉 `bun is a fast`；`npm_install` 带 `with_optional`（平台二进制是 optionalDependency）
- **MiMo Code**：`smoke_cmd: mimo db "SELECT 1"` 真触 SQLite；`smoke_reject: sqlite`

## 2026-10-18 · 真实装机 + 反向假二进制实测

构建物：本仓 `src-tauri/target/debug/u-king-mini.exe`（含本次门禁）。

### ① 真实装机流（`--install-gate-test`，真 npm install）

| 工具 | 结果 | 证据 |
| --- | --- | --- |
| MiMo Code | 装机成功 | 日志：`冒烟：mimo db "SELECT 1"` → `冒烟通过` → `验证通过：0.1.15` |
| OpenCode | 装机失败，无假通过 | 主入口 Bun 壳 + 无可用 baseline，报错而非「安装成功」 |

### ② 反向实测（`--gate-catch-test`，故意塞坏二进制走真实 verify/smoke 路）

| 坏形状 | 门禁拦法 | 结果 |
| --- | --- | --- |
| Bun 壳（`--version` 打 `1.3.14` 退出 0，`--help` 满屏 Bun） | 输出门禁：缺 `opencode` + 触 `bun is a fast` | ✅ 拦下，`install_ok=false` |
| SQLite 挂（`--version` 绿，触库 `SQLITE_CANTOPEN` 退出 1） | 冒烟：`db SELECT 1` 退出 1 | ✅ 拦下，`install_ok=false` |

### ③ 单元测试（`cargo test --lib installer::tests`）

86 项全绿，含新增：

- `output_gate_catches_foreign_tool_fingerprint` — 门禁纯函数判据
- `verify_rejects_bun_shell_even_when_exit_code_is_zero` — 退出码 0 也拦 Bun 指纹
- `verify_runs_smoke_and_fails_when_smoke_fails` — 冒烟失败算验证失败
- `smoke_command_hits_fix_blacklist` — 冒烟命令过危险命令黑名单
- `tool_spec_without_gate_fields_still_parses` — 老清单兼容
- `embedded_manifest_gates_opencode_bun_shell_and_mimo_sqlite` — 内嵌清单必须带门禁

## 跑法

```bash
# 反向实测（本机可重复，不依赖 npm 网络）
src-tauri/target/debug/u-king-mini.exe --gate-catch-test

# 真实装机（真 npm install，较慢）
src-tauri/target/debug/u-king-mini.exe --install-gate-test              # 默认 opencode + mimo-code
src-tauri/target/debug/u-king-mini.exe --install-gate-test mimo-code

# 单元测试
cd src-tauri && cargo test --lib installer::tests
```

## 边界

- OpenCode 的 Bun 壳是上游 `opencode-ai` 包装问题；U-King 只能拒绝假通过、引导 repair/人工，修不了上游包
- MiMo 的 TEMP/TMP 触发机制未确定；`tool_process_env` 会给子进程换干净 TEMP，冒烟验证的是「绕过后能用」，不是「默认 TEMP 不会挂」
