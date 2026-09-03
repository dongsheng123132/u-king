# U 盘 AI 精灵（PicoClaw 内核）× U-King 开发计划

> 2026-09-03 由 sol（gpt-5.6-sol）基于 usb-genie-probe 真机取证报告规划（取证时间
> 2026-09-02 22:45 ~ 09-03 01:25，Windows 11 真机 + SanDisk 3.2Gen1 USB3.x 闪存盘；
> 报告全文在本地任务档案，不入公开仓）。
> 本文件是计划，不是已完成事实；每阶段以真实 U 盘可观察跑通为完成条件。

## 1. 形态决策

选择 **C：U-King 内新增「U 盘工具盘」板块，同时发布由同一制作器生成的绿色便携包**。

具体定义：

- U-King 是唯一产品入口和管理界面。
- 「U 盘工具盘」板块负责选择目标盘、制作、更新、验收和启动 AI 精灵。
- 绿色便携包不是另一套产品，也不拥有独立 UI；它只是同一制作动作预先输出的 `Windows x64` 工具盘目录压缩包。
- 本地安装与 U 盘制作共用同一个动作核心，仅目标目录和展示文案不同。
- PicoClaw 不再作为普通工具重复出现在工具市场，避免给小白提供两个同源入口。

建议发布名：

```text
U-King.exe
USB-AI-Genie-Windows-x64.zip
U-King-Setup.exe
```

用户路径统一为：

```text
U-King
  └─ U 盘工具盘
       ├─ 制作到 U 盘
       ├─ 安装到这台电脑
       ├─ 更新已有工具盘
       └─ 打开 AI 精灵
```

选择 C 的原因：

1. 符合“360 式管家”：用户只需理解 U-King 和一个“制作工具盘”动作，不需要判断应该下载哪个 PicoClaw 入口。
2. 制作器适合已有 U-King 的用户；预组包适合救援、离线准备、批量拷盘，两者覆盖不同交付场景。
3. 两种产物共用同一 Rust 动作核心、运行时清单和验收脚本，不形成双实现。
4. PicoClaw 只是 U-King 延伸到 U 盘上的便携内核，不建立新的品牌、配置中心或更新体系。

一期范围限定为 **Windows x64**。现有取证只覆盖 Windows 11，不应据此宣称 macOS 或其他架构已经可用。

命名定案（fable 会审）：发行包与门面统一叫 **USB AI Genie（U盘 AI 精灵）**；U-King 内板块叫「U 盘工具盘」；发行页贴解压后目录树并说明其中的 U-King.exe 是维护工具盘本体，避免「下错了」的错觉。

---

## 2. 已确认基线

以下数据作为实现和验收基线，不再重新选型或改数：

| 项目 | 已确认事实 |
| --- | --- |
| 内核 | PicoClaw 0.3.1，Go，MIT |
| 上游版本锚点 | tag `v0.3.1`（解引用 `2cf030d2`）；全部源码引用一律带 `ref=v0.3.1`，禁止引用 main（main 已领先 tag 50+ 提交） |
| 压缩包 | 22,385,616 字节 |
| 解压内容 | 62,157,806 字节，约 59.3MB |
| 启动耗时（热缓存 `version` 子命令三次） | 120.3452ms、107.4152ms、92.3664ms；真冷启动未测（37MB exe 按 38.10MB/s 随机读估算为秒级）。对外口径一律「秒级启动」，禁写 0.1 秒 |
| 数据根 | `PICOCLAW_HOME` 可将配置、workspace、session、auth 主路径重定向到 U 盘 |
| 凭据文件 | `<PICOCLAW_HOME>\.security.yml`，API key 为明文 |
| 中文及空格路径 | 已验证可安装、配置、对话和调用工具 |
| 中文用户名 | 未测试 |
| 文件系统 | NTFS 已实测；exFAT/FAT32 未测（32GB+ U 盘出厂默认 exFAT，P0 必须补测 exFAT 真盘） |
| U 盘随机写 | 8.45MB/s |
| U 盘随机读 | 38.10MB/s |
| U 盘顺序读 | 106.34MB/s |
| 已知兼容问题 | `deepseek-v4-flash` thinking 模式多轮 tool-call 触发 `reasoning_content` HTTP 400；v0.3.1 源码显示疑因 provider 字段留空触发主动剥离（非上游缺陷），待 P0 实验定性 |
| C 盘行为 | 取证主链路中 PicoClaw 归属新增、修改均为 0；源码另有 `%TEMP%` 输入历史及少数可选频道的硬编码路径，产品必须主动约束 |

---

## 3. 仓库与目录落位

所有开发文件、清单、测试、文档和发布脚本均进入 u-king 公开仓；该仓是唯一真相源。

### 3.1 前端

新增：

```text
src/
  UsbToolDisk.tsx
  components/
    UsbToolDiskStatus.tsx        # 仅在页面过大时再拆
```

并修改公共汇合点：

```text
src/App.tsx
src/components/Sidebar.tsx
src/i18n/...
```

页面定位：

- 导航名称：`U 盘工具盘`
- 放在侧栏默认可见的维护功能区，不放实验室，也不挤占现有核心四项。
- 页面同时提供“制作到 U 盘”和“安装到这台电脑”。
- 已安装到本机后，“我的 AI”只出现一个“AI 精灵”启动入口。
- 工具市场不再增加独立 PicoClaw 安装卡。

页面至少显示：

- 检测到的可移动盘、卷标、容量、文件系统和剩余空间；
- 目标盘中 U-King、PicoClaw、数据目录的现状与版本；
- 将新增或更新的内容；
- “不会格式化 U 盘、不会删除其他文件”的边界；
- API key 将以明文存入目标盘的醒目提示；
- 制作、更新、验证、打开 AI 精灵四种状态；
- 拔盘、写保护、空间不足、文件占用和校验失败的可理解错误。

### 3.2 Rust 动作核心

新增：

```text
src-tauri/src/usb_genie.rs
```

该模块独占以下业务实现：

- 枚举和校验候选目标盘；
- 解析固定工具盘根目录；
- 下载、校验、解压和原子替换 PicoClaw runtime；
- 生成启动器；
- 创建/合并 `config.json` 和 `.security.yml`；
- 检测已安装版本、数据状态和进程占用；
- 制作到 U 盘或安装到本机；
- 拉起交互式 AI 精灵窗口；
- 验证目录、版本、hash、环境变量和网关配置；
- 更新 runtime 时保留用户数据并支持失败回滚。

公共汇合点只做注册：

```text
src-tauri/src/actions.rs
src-tauri/src/lib.rs
```

建议 Action ID：

```text
runtime.usb_genie.inspect
runtime.usb_genie.deploy
runtime.usb_genie.verify
runtime.usb_genie.launch
runtime.usb_genie.credential_remove
```

动作语义：

- `inspect`：只读，返回候选盘、已装版本、空间、文件系统、阻塞项和 `state_version`。
- `deploy`：同一个动作覆盖首次制作、更新和本地安装；输入为 `target_id`、`target_kind`、`credential_ref`、`expected_state_version`。
- `verify`：只读验证 runtime、配置、重定向和版本，不发起付费模型请求；连通实测由用户明确确认后单独执行。
- `launch`：只启动该目标下的 PicoClaw，不扫描或调用其他同名程序。
- `credential_remove`：删除该工具盘的 `.security.yml` 凭据，属于需确认的破坏性动作。

Action 输入中禁止出现原始 `api_key`。前端只传 `credential_ref`：

```text
official_device
provider:<provider-id>
none
```

`none` = 无凭据模板模式：不生成 `.security.yml`，预组 zip 一律用此模式出包，出包后对 zip 复跑 check-leak。

Rust 后端从已有凭据来源读取，写入目标后立即释放内存中的临时字符串。Action 日志、错误、进度事件和 JSON 输出不得包含完整 key。

`official_device` 使用 U-King 设备钱包 key：制作即把本机钱包的扣费权放进目标盘，UI 必须明示；丢盘处置 = 服务商换 key + `rotate_device_key` 轮换设备 key。

完成动作注册后必须运行：

```bash
pnpm run action-parity:verify
```

`src/generated/` 只由生成器更新，禁止手改。

### 3.3 运行时清单与启动器

新增：

```text
src-tauri/resources/
  picoclaw-runtime.json
  usb-genie/
    launch-agent.cmd
    使用说明.txt
```

`picoclaw-runtime.json` 是版本与供应链信息的唯一源文件，至少包含：

```json
{
  "schema_version": 1,
  "version": "0.3.1",
  "upstream_tag": "0.3.1",
  "upstream_commit": "2cf030d2",
  "platform": "windows-x64",
  "asset_url": "https://github.com/sipeed/picoclaw/releases/download/v0.3.1/picoclaw_Windows_x86_64.zip",
  "sha256": "c76e4b9f3f95137b8cb58229e756538b12ed0fee0e4301a01b531626bf740af1",
  "archive_bytes": 22385616,
  "unpacked_bytes": 62157806,
  "license": "MIT"
}
```

asset URL 与 SHA-256 已于 2026-09-03 实取核验：上游 v0.3.1 资产 `picoclaw_Windows_x86_64.zip`（22,385,616 字节）与本地取证压缩包 SHA-256 一致（本地 sha256sum 复算 = 上表值）；发布前仍须对 Release 页资产再复核一次。换版本必须重取重核。

### 3.4 构建、验收与发布脚本

新增：

```text
scripts/
  build-usb-tool-disk.mjs
  check-picoclaw-bundle.mjs
  check-usb-genie-launcher.mjs

tools/
  usb-genie/
    smoke.ps1
    probe-c-drive.ps1
    fixtures/
      runtime-manifest.json
      security-masked.yml

docs/
  usb-ai-genie.md
```

职责：

- `build-usb-tool-disk.mjs` 不复制业务逻辑；它调用 U-King CLI 的 `runtime.usb_genie.deploy` 动作，把同一制作器输出到临时目录后再打 zip。
- `check-picoclaw-bundle.mjs` 校验版本、hash、文件清单（runtime 只允许 picoclaw.exe / LICENSE / README.md）、LICENSE、NOTICE、启动器和禁止项；`picoclaw-launcher.exe` 明确禁止进包（24MB WebUI 启动器占运行时约 40%，其落盘行为无取证证据）。
- `check-usb-genie-launcher.mjs` 静态确认启动器设置了全部必要环境变量，且没有硬编码盘符、用户名和 key。
- `smoke.ps1` 用于 Windows 真盘冒烟测试。
- `probe-c-drive.ps1` 沿用取证报告的轻量方法：顶层对比加可疑目录钻取，不再执行数百万文件的全量递归快照。

现有 `scripts/pack-usb.sh` 后续改为调用同一个制作动作，不能继续维护第二套目录组装规则。

---

## 4. 工具盘目录布局

建议工具盘根目录如下：

```text
<U盘根>\
  U-King.exe
  启动 AI 精灵.cmd
  使用说明书.html

  U-King\
    icon.ico
    uking.key                       # 仅现有 usb-guard 发行形态需要
    AI-Genie\
      install.json
      current.json

      runtime\
        picoclaw-0.3.1\
          picoclaw.exe
          LICENSE
          README.md

      data\
        config.json
        .security.yml
        workspace\
        logs\
        tmp\

      rollback\
      staging\
```

约束：

- `runtime` 是不可变程序区，按版本隔离。
- `data` 是用户区，首次创建后更新不得覆盖。
- `staging` 只用于同盘下载后解压和验收；成功后原子改 `current.json` 或重命名目录。
- `rollback` 最多保留上一版 runtime，不复制用户数据。
- `.security.yml` 永远位于 `data` 中，不进入 zip 模板、发布产物、日志或测试证据。
- 更新只替换 runtime、启动器和版本元数据，不重跑 `onboard` 覆盖 workspace。
- 不向 U 盘根目录散落 PicoClaw 内部文件。
- 用户已有文件不得删除、移动或改名；制作器不得提供格式化功能。

本地安装采用同构布局：

```text
<installer::uking_home()>\usb-genie\
  runtime\
  data\
  rollback\
  staging\
  current.json
```

这样本地安装与 U 盘制作可以复用同一套路径和更新逻辑。

---

## 5. 启动器设计

### 5.1 环境变量

启动器只能影响 PicoClaw 子进程，不设置系统级或用户级环境变量，不修改 PATH。

核心逻辑：

```bat
@echo off
setlocal
set "GENIE_ROOT=%~dp0U-King\AI-Genie"
set "PICOCLAW_HOME=%GENIE_ROOT%\data"
set "PICOCLAW_CONFIG=%PICOCLAW_HOME%\config.json"
set "PICOCLAW_BINARY=%GENIE_ROOT%\runtime\picoclaw-0.3.1\picoclaw.exe"
set "PICOCLAW_BUILTIN_SKILLS=%PICOCLAW_HOME%\workspace\skills"
set "PICOCLAW_LOG_FILE=%PICOCLAW_HOME%\logs\picoclaw.log"

rem PicoClaw reads input history from os.TempDir(); keep TEMP/TMP on the
rem tool disk so prompt history never lands on the host %TEMP%.
set "TEMP=%PICOCLAW_HOME%\tmp"
set "TMP=%PICOCLAW_HOME%\tmp"

if not exist "%PICOCLAW_HOME%\workspace" mkdir "%PICOCLAW_HOME%\workspace"
if not exist "%PICOCLAW_HOME%\logs" mkdir "%PICOCLAW_HOME%\logs"
if not exist "%PICOCLAW_HOME%\tmp" mkdir "%PICOCLAW_HOME%\tmp"

cd /d "%PICOCLAW_HOME%\workspace"
"%PICOCLAW_BINARY%" agent
set "EXIT_CODE=%ERRORLEVEL%"

if not "%EXIT_CODE%"=="0" (
  echo.
  echo AI Genie exited with code %EXIT_CODE%
  pause
)
cd /d "%SystemDrive%\"
exit /b %EXIT_CODE%
```

正式模板不能把版本号写死在多处。制作动作根据 `current.json` 渲染 runtime 路径，或生成一个稳定的：

```text
runtime\current\picoclaw.exe
```

其中 `current` 由同盘原子目录切换维护。

启动器 `.cmd` 正文只用 ASCII（installer.rs:897 教训：cmd.exe 按系统 ANSI 代码页解析批处理，UTF-8 中文 echo 在 GBK 控制台乱码）；中文只允许出现在文件名。`PICOCLAW_BUILTIN_SKILLS` 显式设置，不依赖 cwd 碰巧命中。

### 5.2 从 U-King 启动

Rust 使用：

```text
cmd.exe /d /c "<目标>\启动 AI 精灵.cmd"
```

Windows 下显式使用 `CREATE_NEW_CONSOLE`，这是用户要求的交互窗口，不使用 `CREATE_NO_WINDOW`。HTTP 下载与后台检查仍使用隐藏窗口和超时。

发布 zip 里的 .cmd/.exe 带 Mark-of-the-Web，首次双击会弹 SmartScreen——P1 在使用说明与发行页写明处理方式；代码签名放 P2。

启动前检查：

- 目标盘仍在线；
- runtime hash 正确；
- `.security.yml` 存在；
- workspace 可写；
- 没有未完成的 staging；
- 版本路径与 `current.json` 一致。

只能管理由本动作启动并登记 PID、可执行文件完整路径的进程；不得按 `picoclaw.exe` 镜像名批量杀进程。

### 5.3 源码中发现的旁路落盘点

仅设置 `PICOCLAW_HOME` 仍不足以承诺所有可选功能绝不触碰宿主机：

- 输入历史默认走 `%TEMP%\.picoclaw_history`；
- credential 加密功能可能使用 `~/.ssh`；
- DeltaChat、企业微信频道存在硬编码用户目录路径；
- exec 工具在 Windows 把命令中的 `~` 展开为宿主主目录（`pkg/tools/shell.go:1192`，ref=v0.3.1）；
- MCP 配置同理（`pkg/mcp/manager.go:34`）。

因此 P0/P1：

- 同时重定向子进程 `TEMP`、`TMP`；
- 只开放经过取证的 CLI agent、workspace 和工具调用主链路；
- 暂不开放 credential 加密、MCP 配置、DeltaChat、企业微信频道；
- 若未来开放，必须先补独立落盘取证或完成上游修复。

---

## 6. API key 与配置安全

### 6.1 写入方式

生产实现不得执行带 `--api-key <明文>` 的命令，因为参数可能出现在进程列表、诊断工具或 shell 历史中。

由 Rust 后端直接生成或结构化合并：

```text
data\config.json
data\.security.yml
```

配置效果应与已取证的 PicoClaw `model add` 结果一致：

```json
{
  "model_name": "xiapan-v4-flash",
  "provider": "",
  "model": "deepseek-v4-flash",
  "api_base": "<当前官方网关地址>",
  "enabled": true
}
```

```yaml
xiapan-v4-flash:0:
  api_keys:
    - <完整 key，仅此处存在>
```

网关一律用 `https://api.u-claw.org.cn/v1`（境内直连）。取证报告里 curl 通过的 `api.u-claw.org` 是境外地址——u-king 自己的 CI 明令禁止它进 providers.rs（`.github/workflows/ci.yml:50`），`device.rs:143` 注明裸网被 reset；当时能通是开发机常驻代理所致，不构成可用性证据。P0 手工链路必须先用 cn 域名重跑一轮再固化。P1 不在 `usb_genie.rs` 复制网关常量，从 U-King 现有 provider 模板读取当前官方端点。

写入流程：

1. 从已有 `credential_ref` 解析 key；
2. 在同目录生成随机临时文件；
3. 写入并 flush；
4. ACL 仅在本机安装形态设为仅当前用户可读；U 盘形态跳过（可移动盘换机后 SID 不匹配，agent 自己都读不了凭据），并在 UI 说明；
5. 同盘原子替换 `.security.yml`；
6. 内存变量立即离开作用域；
7. 日志只记录“凭据已配置”和打码标识；
8. UI、Action 返回和遥测不返回 key。

测试夹具只允许使用明显虚假的 `sk-abc123`。

### 6.2 隐私边界

PicoClaw 0.3.1 的 `.security.yml` 保存明文 key，这是产品必须明示的事实：

- NTFS 上可尽力设置 ACL，但 U 盘换到另一台机器后不能把 ACL 当作可靠保护。
- FAT32/exFAT 不提供等价的文件 ACL。
- 拿到 U 盘并能读取文件的人，可能读取 API key、对话、workspace 和会话数据。
- “弹出 U 盘”不能吊销 key；丢盘后的真正补救是服务端换 key。
- 页面必须提供“移除此盘凭据”按钮，并说明丢盘后应在 U-King 中轮换原 key。
- 不得用“已加密”“安全存储”等文案掩盖明文落盘事实。

---

## 7. 版本检测与升级

### 7.1 检测

`runtime.usb_genie.inspect` 同时读取：

- `current.json` 的声明版本；
- `picoclaw.exe version` 的真实输出，5 秒超时；
- runtime 文件 SHA-256；
- 内嵌 `picoclaw-runtime.json` 的目标版本；
- 可选的线上稳定清单版本。

只有声明版本、真实版本和 hash 三者一致，才报告 `ready: true`。

线上清单失败时使用内嵌清单，不阻塞现有工具盘启动。远程清单只能选择 U-King 已支持并携带 hash 的版本，不能让网络返回任意可执行 URL。

### 7.2 更新

更新流程：

1. 检测剩余空间和文件系统；
2. 下载到本机缓存或目标盘 `staging`；
3. 校验固定 SHA-256；
4. 解压到新的版本目录；
5. 校验必需文件和 MIT LICENSE；
6. 执行 `picoclaw.exe version`；
7. 生成新启动器；
8. 将旧 runtime 标为 rollback；
9. 原子切换 `current.json`；
10. 重新执行只读验证；
11. 失败则切回旧 runtime；
12. 始终保留 `data`。

本机缓存只能保存不可变安装包和 hash，不得缓存 key、session、workspace 或提示词历史。

### 7.3 上游跟踪

P2 增加定时工作流：

```text
.github/workflows/picoclaw-upstream.yml
```

规则：

- 每周检查 PicoClaw 官方 release；
- 发现高于当前固定版本时只生成报告或维护 issue；
- 不自动改清单、不自动合并、不自动发布；
- 升级前必须核对许可证、release asset、SHA-256、解压文件和 changelog；
- 重跑中文路径、落盘、网关、工具调用和 thinking 多轮判据；
- 人工确认后再更新唯一的 `picoclaw-runtime.json`。

若将来需要修改 PicoClaw，优先向上游提交补丁；仓内如暂存补丁，必须记录上游提交、补丁文件、构建方法和新增许可证义务。

---

## 8. 分阶段计划

## P0：手工链路和判据固化

建议预算：**4～6 小时**。

### 交付物

- `docs/usb-ai-genie.md` 的手工制作说明；
- 启动器原型；
- `tools/usb-genie/smoke.ps1`；
- 轻量 C 盘落盘检查脚本；
- 脱敏测试日志模板；
- thinking 400 的识别文案和兜底规则；
- 工具盘目录原型。

### 执行链路

1. 将已取证 PicoClaw 0.3.1 压缩包解压至固定 `runtime` 目录。
2. 创建 `data/workspace`、`data/logs`、`data/tmp`。
3. 设置 `PICOCLAW_HOME`、`PICOCLAW_CONFIG`、`PICOCLAW_BINARY`、`PICOCLAW_LOG_FILE`、`TEMP`、`TMP`。
4. 运行 `onboard`。
5. 使用一次性测试 key（cn 域名网关），将网关配置落入 `config.json` 和 `.security.yml`；测试 key 用后即从服务端删除、本地明文即删。
6. 通过启动器拉起交互式 `picoclaw agent`。
7. 完成单轮问答。
8. 完成写文件与执行命令的工具调用。
9. 退出后检查 workspace、session、日志、历史和凭据均位于目标目录。
10. 使用轻量方法检查宿主机可疑路径。
11. 在中文加空格的盘内目录重跑。
12. **P0 第一实验**：把 config.json 模型条目的 `"provider": ""` 改为 `"provider": "deepseek"` 重跑多轮 tool-call 对话（v0.3.1 `requiresToolRoundReasoningReplay()` 只对 provider=deepseek 或 DeepSeek 官方域名回传 reasoning_content；取证时的 400 疑为字段留空触发主动剥离）。实验前先核实 provider 名不改动 api_base 与鉴权路径。第二兜底：`thinking_level: "off"`（`config.go:780`）。
13. 在 exFAT 真盘重跑最小链路（onboard + 单轮对话 + 写文件 + 执行命令）。
14. 记录发布 zip 的 MOTW/SmartScreen 实际行为、启动器在 GBK 控制台的编码表现。

### 验收判据

- `picoclaw.exe version` 报告 0.3.1。
- 解压文件总量与 62,157,806 字节基线一致，约 59.3MB。
- 启动器不依赖盘符，不写用户级环境变量。
- 单轮对话成功。
- `write_file` 和 `exec` 各成功一次。
- `config.json`、`.security.yml`、workspace、session、日志、临时历史均位于工具盘目录。
- 报告和脚本输出中不存在完整 key。
- 中文及空格目录成功。
- thinking 400 按 §8 P0 第 12 步实验定性；若为配置问题则不再以「兼容缺陷」口径出现在任何文案。
- provider=deepseek 实验结论落盘：400 消失则 §9 缩为回归 fixture。
- exFAT 真盘最小链路成功（或明确 blocker 登记到 P1）。
- 未通过的项目必须保留为 P1 blocker，不能用“基本可用”放行。

## P1：U-King 一键制作与本地安装

建议预算：**14～16 小时**（fable 估全量实现 25～35 小时，必须按上述砍法执行；超支即按 §13 冻结纪律延后非核心项，不赶工）。

### 交付物

- `UsbToolDisk.tsx` 页面；
- `usb_genie.rs` 动作核心；
- ActionParity 动作；
- runtime 固定清单；
- 制作、验证、启动链路。**砍**：staging/rollback/current.json 原子切换（P1 的「更新」= 重制新版 runtime 目录、data 原样保留；原子切换三件套 P2）；
- **砍**：「安装到这台电脑」UI 入口（动作核心保留 `target_kind` 参数，按钮以后一小时补上）；
- **砍**：launch 的 PID 登记与进程管理（P1 只 spawn 不管理；「禁止按镜像名杀进程」纪律不变）；
- 同制作器生成的预组绿色 zip（credential_ref=none 出包）；
- NOTICE、隐私文档和使用说明更新（含 SmartScreen 首跑说明、SHA-256 核对方法）；
- Rust 单测、脚本检查和真实 U 盘冒烟（NTFS + exFAT 各一块，含非空盘测试）。

### 验收判据

- U-King 能识别可移动盘并显示卷标、文件系统、空间和现有版本。
- 不提供格式化操作，不覆盖目标盘其他目录。
- 对同一目标重复执行 `deploy` 是幂等的。
- 拔盘或写入失败不会留下“已完成”的假状态。
- 重制更新不覆盖 `data`、workspace、session 和 `.security.yml`（P1 更新语义 = 新版本目录 + data 保留）。
- 本地安装（CLI 形态）与 U 盘制作调用同一个动作核心。
- 预组 zip 由 U-King CLI Action 生成，不存在第二套组装逻辑。
- U-King 可拉起独立交互窗口，关闭 U-King 后该窗口行为有明确设计和测试。
- action 输入、输出、日志和进度事件均不泄露 key。
- NTFS 与 exFAT 真盘各一块覆盖（exFAT 是 32GB+ U 盘出厂默认，用户主力场景）；FAT32 未覆盖 至少覆盖两种文件系统；未覆盖者必须在界面声明。
- 至少完成一次非空 U 盘测试，证明既有文件不受影响。
- 中文用户名场景若仍未完成，不能宣称全面兼容，须保留显著已知限制。
- `pnpm run action-parity:verify` 通过。
- 公开仓三道闸门全部通过：

```bash
node scripts/check-leak.mjs .
pnpm build
cd src-tauri && cargo test --lib
```

Rust 测试不得少于 main 当前数量（2026-09-03 时点 512 个），并应增加 usb_genie 专项用例。

## P2：增量更新、多盘与供应链加固

建议预算：**12～20 小时，决赛后实施**。

### 交付物

- 基于 hash 的增量更新或整包复用；
- 多盘并发状态管理；
- 卷 GUID/序列身份识别；
- 上游 release 定时跟踪；
- 可选清单签名和发布签名；
- 更完善的回滚、断点续传和弹盘处理；
- 中文用户名干净机回归；
- 多种低速 U 盘体验测试。

### 验收判据

- 盘符变化后仍能通过卷身份找到原目标。
- 两块盘同时插入时，进度、版本和错误不串盘。
- 同一盘不允许并发执行两个写动作。
- 下载中断、拔盘、空间耗尽、hash 错误均不会损坏当前可用版本。
- 签名开启后，签名错误必须阻断安装；签名关闭时仍强制校验固定 SHA-256。
- 上游发现新版本不会自动进入稳定发布。
- 中文用户名下完成制作、配置、启动、工具调用、更新和卸载凭据的完整回归。

---

## 9. thinking 模式多轮 400 的产品兜底

**归因待定（P0 第一实验定性）**：v0.3.1 源码显示这大概率不是上游缺陷而是配置问题——`requiresToolRoundReasoningReplay()` 仅在 provider 名为 `deepseek` 或 api_base 为 DeepSeek 官方域名时回传 `reasoning_content`，否则调用 `stripReasoningMessages` 主动剥离（取证配置 provider 留空 + 虾盘云域名 → 剥离 → 网关 400）。若 P0 实验证实，本节其余条款缩成一条回归 fixture。任何情况下不得归因于网络或 API key。

识别条件：

```text
HTTP 400
reasoning_content
thinking mode must be passed back
```

兜底策略：

1. P0 先跑 P0 第 12 步两实验（provider=deepseek / thinking_level=off）；任一成立则本节其余产品兜底条款不生效。
2. 若网关能提供兼容的非 thinking 路由，优先由现有 provider 模板选择该路由，不在启动器里造模型别名。
3. 捕获上述特征错误时，提示：“当前模型的多轮思考记录与 PicoClaw 0.3.1 不兼容；单轮对话仍可用。”
4. 提供“开启新会话继续”按钮或明确命令。
5. 已经执行过工具调用后，不得自动重放整段任务，避免重复写文件、发送请求或执行命令。
6. 只有能证明失败发生在任何工具副作用之前，才允许自动新会话重试一次。
7. 不无限重试，不把 400 包装成 key 过期。
8. 将复现消息序列做成脱敏 fixture，加入回归测试。
9. 长期修复方向为上游补齐 `reasoning_content` 回传或在兼容层规范化消息，不在产品文案中承诺尚未完成的修复。

---

## 10. 性能与体验预期

随机写只有 8.45MB/s，产品不能把 U 盘表现描述成“和本机一样快”。

设计要求：

- 首次制作按 59.3MB 解压体积加校验展示真实进度。
- 大文件顺序写，避免逐个同步大量小文件。
- 更新先构造新 runtime，禁止对现有目录逐文件原地覆盖。
- workspace、session 和凭据必须留在 U 盘，不能为了速度静默迁到本机。
- 本机缓存只用于不可变下载包。
- UI 文案说明：程序启动约 100ms，但首次模型回复主要受网络影响；会话和大量小文件操作受 U 盘随机写速度影响。
- 进度超过 2 秒必须显示当前阶段；超过合理超时需给出可重试错误，不能保持假“进行中”。
- 低速盘上优先保证不损坏数据，不以激进并发换取表面速度。

---

## 11. 开源与合规

### 11.1 许可证

- U-King 主项目继续使用 Apache-2.0，根 `LICENSE` 不改成混合许可证。
- 根 `NOTICE` 的第三方组件章节增加 PicoClaw：
  - 项目名称；
  - 官方仓库地址；
  - 固定版本 0.3.1；
  - MIT；
  - 上游 LICENSE 中的原始版权归属，必须原样核对后填写，不凭记忆编写。
- 每个工具盘 runtime 内保留 PicoClaw 原始 `LICENSE`。
- 使用说明和发布页说明 PicoClaw 是第三方 MIT 组件，U-King 与启动器仍按本项目许可证发布。
- 如修改上游代码，发布产物必须能对应到公开源码、补丁和构建步骤。

### 11.2 泄漏纪律

- 不提交真实 key、真实客户路径、设备编号、主机名或测试原始日志。
- 文档和 fixture 使用 `example.com`、`user1`、`sk-abc123`。
- `.security.yml` 只允许以打码或假 key 形式出现在测试夹具。
- `check-leak` 新增针对 PicoClaw 凭据结构的检查样例，但白名单不得宽泛放开 `sk-`。
- Action 审计、故障包和遥测统一复用现有脱敏能力。

### 11.3 CI 增补

普通 PR CI 增加：

- runtime 清单 schema 校验；
- hash 字段格式校验；
- 启动器环境变量静态检查；
- 禁止启动器硬编码盘符和用户名；
- 工具盘模板必须包含 PicoClaw LICENSE；
- `.security.yml` 不得进入发布模板；
- usb_genie Rust 单测；
- ActionParity 清单无漂移；
- Linux CI 仅编译平台分支，不声称执行过 Windows 真盘测试。

发布工作流增加：

- 下载固定上游 asset；
- 校验 SHA-256；
- 解压后核对必需文件和版本；
- 用 U-King CLI Action 组装工具盘；
- 再次扫描泄漏；
- 生成 zip 和 SHA-256；
- 运行 Windows 工具盘冒烟；
- 上传 `USB-AI-Genie-Windows-x64.zip`（P1 由本机构建后手动 `gh release upload` 到门面仓；windows-latest 自动构建与跨仓自动上传依赖尚不存在的 Windows CI 作业与细粒度 PAT，放 P2）。

防僵尸：u-king 每次发版的发布脚本增加一步只读校验——发行门面仓存在同 tag 的 release，否则本次发布判失败（门面仓只有被发版流程反复触碰才不会死）。

---

## 12. 主要风险清单

| 风险 | 影响 | 对策 |
| --- | --- | --- |
| thinking 多轮 tool-call HTTP 400 | 长任务中断 | 特征识别、新会话继续、禁止有副作用后的盲重放、保留上游修复路线 |
| U 盘随机写仅 8.45MB/s | 解压、session 和小文件操作变慢 | 顺序写、整目录切换、真实进度、明确体验预期 |
| 中文用户名未测 | 用户目录、TEMP、命令行引用可能失败 | P1 标已知限制，P2 用中文用户名干净 Windows 完整回归 |
| `.security.yml` 明文 key | 丢盘或借盘造成凭据泄漏 | 醒目告知、最小 ACL、凭据移除与轮换入口、日志永不回显 |
| `%TEMP%` 输入历史 | 提示词可能留在宿主机 | 子进程同时重定向 TEMP/TMP 到工具盘 |
| 可选频道硬编码用户目录 | 破坏“数据随盘”承诺 | P0/P1 禁用未取证频道，开放前补证据或上游修复 |
| 制作过程中拔盘 | 半成品或目录损坏 | staging、完成标记、原子切换、再次插入可恢复 |
| 盘符变化 | 更新或启动找错盘 | P1 操作期间固定 canonical target；P2 使用卷 GUID/序列 |
| 非空 U 盘误覆盖 | 用户数据损失 | 只写固定子树，不格式化，不递归清理根目录，目标越界检查 |
| 下载包被篡改 | 执行恶意二进制 | 固定 SHA-256、版本实测、P2 可选签名 |
| runtime 正在运行时更新 | 文件锁或半更新 | 识别本模块登记 PID，提示退出；不按镜像名强杀 |
| 在线清单失效 | 无法制作或误判版本 | 内嵌固定清单兜底，线上清单只能推荐已验证版本 |
| 预组包与制作器漂移 | 两种形态行为不一致 | 预组包必须由同一 Action Core 生成 |
| Action 日志泄露 key | 公开日志或故障包泄密 | 只传 credential reference，输出与进度不含 secret |
| 文件系统差异 | ACL、原子替换行为不一致 | 分别测试 NTFS、FAT32/exFAT；明确安全能力差异 |
| PicoClaw 上游接口变化 | 新版启动或配置失效 | 固定版本，定时只报告，不自动升级 |

---

## 13. GOAI 决赛前时间预算

GOAI 决赛时间为 2026-09-22。此线在决赛前建议设置 **最多 24 小时硬预算**，不抢占决赛主线。

建议节奏：

| 时间 | 工作 | 预算 |
| --- | --- | ---: |
| 9月3日～5日 | P0 手工链路、启动器、判据 | 4～6小时 |
| 9月7日～12日 | P1 Rust 动作核心和前端板块（按 §8 砍后范围） | 12～14小时 |
| 9月13日 | 真盘验收、合规、三闸门 | 4小时 |
| 9月14日～22日 | 功能冻结，只修阻断性问题 | 最多2小时缓冲 |
| 9月23日以后 | P2 增量更新、多盘、签名、中文用户名干净机 | 12～20小时 |

执行纪律：

- 决赛前只交付 Windows x64 的最小完整闭环。
- 不在 P1 顺手开发聊天 UI、频道系统、技能市场或 PicoClaw 深度改版。
- P0 不通过则不进入 UI 美化。
- P1 到 9月13日仍未完成时，保留手工链路与开发预览，不仓促发布带凭据风险的半成品。
- P2 全部后移，不以“顺手做完”为由侵占决赛准备时间。
- 公共汇合点由一个终端串行修改；需要与其他开发并行时使用独立 worktree。
- 每个阶段都以真实 U 盘可观察跑通为完成条件，构建成功不能替代真盘验收。

---

## 14. 发布完成定义

只有同时满足以下条件，才能宣布“U 盘 AI 精灵已交付”：

- U-King 内有唯一、清晰的「U 盘工具盘」入口；
- 能制作到非空真实 U 盘且不影响既有文件；
- 能安装到本机，并与 U 盘形态共用动作核心；
- 能从目标目录打开交互式 PicoClaw agent；
- 网关单轮对话、写文件和执行命令均真实成功；
- runtime、配置、凭据、workspace、session、日志和交互历史均按产品边界落盘；
- 完整 key 未进入命令行、日志、Action 输出、故障包、仓库或发布模板；
- 更新失败可回滚，用户数据不被覆盖；
- thinking 400 能被准确识别和安全处理；
- 绿色便携 zip 由同一制作器生成；
- PicoClaw MIT LICENSE 与 NOTICE 归属完整；
- `action-parity:verify` 通过；
- 公开仓三道提交闸门全部通过；
- Windows 真盘冒烟记录使用假 key 或完整脱敏；
- 发布 zip、U-King 可执行文件和 runtime 均有 SHA-256；
- 未验证的中文用户名、文件系统或平台场景在文档中明确标为未验证，不扩大宣传口径。
