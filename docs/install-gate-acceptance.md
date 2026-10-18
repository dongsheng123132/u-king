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

### ④ 提交后复测（`c99fc95f1` 之后，2026-10-18）

同机重跑 `--install-gate-test`，确认入库后门禁链路无回归：

| 工具 | 结果 | 证据 |
| --- | --- | --- |
| MiMo Code | ✅ 装机成功 | `冒烟：mimo db "SELECT 1"` → `冒烟通过` → `验证通过：0.1.15` |
| OpenCode | ✅ 失败但无假通过 | npm EEXIST 后 repair 重装仍报「主入口启动异常 + 无 baseline」，**不是**「安装成功」 |

OpenCode 坏壳是上游 `opencode-ai` 包装问题（平台二进制 optionalDependency / baseline 不可用），
U-King 侧只能拒绝假通过，暂不追上游。

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

## 遗留项

### L1 · OpenCode 装出 Bun 壳（上游 `opencode-ai` 包）— 未解决，暂搁置

**现象**：`npm install -g opencode-ai` 装完后，`opencode` 实际是 Bun 运行时壳，不是 OpenCode。

| 命令 | 坏现场 | 健康现场 |
| --- | --- | --- |
| `opencode --version` | `1.3.14`（Bun 版本）退出 0 | `1.19.3`（OpenCode 版本） |
| `opencode --help` | `Bun is a fast JavaScript runtime…` | OpenCode 帮助（含 `opencode` 字样） |

**复现步骤**（Windows x64，2026-10-18 本机实测）：

1. `npm install -g opencode-ai`（或走 U-King 装机 OpenCode）
2. `opencode --version` → 若打出 `1.3.14` 而非 `1.19.3`，即是坏壳
3. `opencode --help` → 若是 Bun 帮助（`Bun is a fast JavaScript runtime`），确认
4. 看包内入口：`node_modules/opencode-ai/bin/opencode.exe`（坏壳约 180MB，实为 Bun 编译产物）

**根因（上游包结构）**：

- 真正的平台二进制在 optionalDependency 里（`opencode-windows-x64` / `opencode-windows-x64-baseline` 等）
- `postinstall.mjs` 负责把平台二进制拷到 `bin/opencode.exe`；optional 依赖被镜像跳过或 postinstall 失败时，留下的就是 Bun 壳
- `postinstall` 自己的 `verifyBinary()` **也只看退出码**，Bun 壳 `--version` 退出 0 → 它也误判成功
- 尝试用 `opencode-windows-x64-baseline` 替换时，该包同样可能缺失/损坏 → 报「主入口启动异常，且没有通过验证的同版本 baseline 兼容包」

**U-King 侧已做（只能到这）**：

- `npm_install` 带 `with_optional: true`，尽量拉齐平台二进制
- 输出门禁钉死 `bun is a fast`，坏壳不再报「安装成功」
- `ensure_opencode_windows_compatibility` 尝试用同版本 baseline 替换坏入口

**阻塞原因**（为什么搁置）：

- 坏壳由上游 `opencode-ai` 的打包/postinstall 逻辑决定，U-King 改不了 npm 包本身
- npmmirror 对 optionalDependency 的同步策略不受我们控制，`--include=optional` 不能保证一定拉到
- baseline 替换依赖 `opencode-windows-x64-baseline` 同版本可用，该包缺失时无解
- 需要上游修 postinstall 验收（不能只看退出码）或改包结构，才能根治

**暂定处置**：门禁拒绝假通过 → 用户看到明确失败并引导重试/换网络；不追上游。

### L2 · MiMo 默认 TEMP/TMP 下 SQLite 初始化失败 — 根因未定

**现象**：同一程序、同一数据库，默认 `TEMP/TMP` 报 SQLite 错误；换成新临时目录可启动。

**U-King 侧已做**：`tool_process_env` 给 mimo 子进程换干净 TEMP（`~/.uking/tmp/mimo`）绕过；冒烟 `mimo db "SELECT 1"` 验证绕过后可用。

**阻塞原因**：触发机制未确定（怀疑与 Windows 默认 TEMP 权限/路径长度/杀软有关），只能绕过不能根治。冒烟验证的是「绕过后能用」，不是「默认 TEMP 不会挂」。

## 边界

- 上述遗留项完成前，OpenCode 在部分客户机上可能装失败（失败是正确行为，好过假成功）
- MiMo 在 U-King 终端内可用（TEMP 已绕过），用户自开外部终端若走默认 TEMP 仍可能踩 L2
