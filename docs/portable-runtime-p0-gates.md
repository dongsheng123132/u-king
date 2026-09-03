# 便携 AI 运行时 P0 发布闸门

> 适用于 PicoClaw 首个适配器；OpenClaw、ClawX 接入同一界面前也必须逐项满足。
> 未完成本文件所有 P0 项时，U-King 的「U盘工具盘」禁止开放更新和凭据写入。
> 首次制作可以开放，但前提是固定 runtime 已在宿主缓存中完成大小与 SHA-256 校验、目标身份和
> 所有权已在第一次写盘前核验、并且写入只进入同盘 staging 与受管目录。
> 例外是已经由本版本 `verify` 成功验证过的既有实例：可提供“启动已验证的 AI 精灵”，
> 但启动前必须再次 verify，不能把它表述成已具备完整制作/升级能力。

## 动作契约

- `inspect` 返回稳定 `target_id`（卷 GUID/序列号）、显示盘符、卷标、文件系统、容量、余量、只读状态，
  以及 `inventory_state_version` 和每个目标的 `target_state_version`；盘符不是身份。
- 写动作接收 `target_id`，执行前重新解析身份；相对路径、UNC、子目录、`C:\\`、换盘符后被接管的盘、
  reparse point/junction/symlink 一律拒绝。
- 全局库存陈旧和目标陈旧都返回明确 conflict，不能用一个模糊 `state_version` 混用。
- PicoClaw P1 凭据引用只允许 `none | official_device`；未实现的 provider 不进入公开 schema。

## 写入与所有权

- 首次写入前完成输入、身份、空间、文件系统、压缩包 hash、凭据可解析性及所有权的全部检查。
- `U-King\\AI-Genie` 已存在但没有合法 `install.json` 所有权标记时拒绝；根启动器只替换已知模板或
  已归属当前实例的文件。
- 全部新 runtime 在同盘 staging 内验证后才提交；完成标记最后写入。任一失败、拔盘或占用后，旧版本仍可验证、启动。
- 更新不得触碰 `data/workspace`、会话、日志或 `.security.yml`。`credential_ref=none` 在更新时保留已有凭据；
  移除凭据只能由单独、确认过的 destructive action 完成。

## 可验证性与启动

- `verify` 对落盘的 exe、LICENSE、README、启动器和配置复算实际 hash；不能只信 `current.json` 声称的 hash。
- 下载、解压、版本探测均有真实子进程超时与终止；动作清单中的 timeout 不是实现。
- `launch` 先 verify，且登记目标级进程身份；重复调用返回 already-running，不得反复开多个 agent。
- action 输出、进度、错误、审计和故障包均不含完整 key。

## 真盘验收

- NTFS 与 exFAT 各验一块，至少一块带既有文件；制作前后既有文件 hash 完全一致。
- 覆盖中文/空格路径、非管理员、空间不足、写保护、提交中拔盘、运行中更新、换盘符。
- 退出后检查运行时的 config/workspace/session/log/tmp 都在所选盘，宿主敏感目录无新增。
- 绿色包、日志、临时文件和仓库完成泄漏扫描；通过 ActionParity、构建和完整 Rust 回归后才允许发布。

## 跨 Windows / Mac 的 exFAT 边界

- exFAT 是**资料盘**的共同文件系统：workspace、会话、日志和受管 runtime 文件可被两端看见；
  它不是可执行文件跨平台的承诺。
- 当前 PicoClaw 适配器只含 `windows-x64` runtime 与 `.cmd` 启动器。因此 Windows 制作的工具盘
  插到 macOS 上只能作为资料盘，绝不能显示“打开 AI 精灵”或声称 Mac 已支持。
- macOS 支持必须新增独立的 `macos-arm64`（必要时 universal）固定清单、SHA-256、可执行权限、
  `.command`/App 启动器与 macOS Action adapter；不得让 Mac 调用 Windows Action 的盘符枚举或启动逻辑。
- Mac 真机验收至少覆盖：识别一块真实 external physical exFAT 盘、首次制作、拔插后重发现、
  从 U-King Mac 端和根启动器各启动一次，并证明资料仍在同一盘且宿主 `$TMPDIR` 无 PicoClaw 状态泄漏。
