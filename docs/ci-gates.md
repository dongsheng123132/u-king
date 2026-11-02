# CI 门禁清单（2026-11-02 起）

`.github/workflows/ci.yml` 的每个 job 各守一段。本文说明每段"证明什么"、为什么按平台拆、
以及 clippy 这笔存量账怎么在**不把 main 打红**的前提下收口。

## Job 地图

| job | runner | 跑什么 | 红意味着 |
|---|---|---|---|
| `static-checks` | ubuntu | 泄漏闸门 `scripts/check-leak.mjs`、版本号四处一致、关键修复防回退 grep | 敏感信息进仓、发版版本号没同步、历史 bug 被回退 |
| `frontend` | ubuntu | `pnpm install --frozen-lockfile` → `pnpm exec tsc --noEmit` → `pnpm build` | lockfile 与 package.json 漂移、TS 类型错、静态自检/vite 构建失败 |
| `rust` | ubuntu | `cargo check`、`cargo clippy`（只报错误不卡 warning）、action-parity 清单未漂移 | 编译失败 / 影核生成文件漂移 |
| `clippy-baseline` | ubuntu | `cargo clippy --all-targets` → 与 `clippy-baseline.json` 比对；并断言 baseline 这次改动只减不增 | **新代码引入了新 warning**，或有人把 baseline 抬高 |
| `rust-tests` | windows | `cargo test --lib`（614 条 `#[test]`，全在 lib；跳过 1 条 locale 依赖用例，见下） | Rust 单元测试红 |

**为什么测试跑 Windows**：这批测试写的是 Windows 语义——`discover_tools_from` 在非 Windows
只找无扩展名文件、`unwrap_shim` 解的是 `.cmd` 壳、用例里直接写 `%APPDATA%\npm` 和
`C:\Users\...`。同一个 HEAD 在 macOS 上是 561 passed / 9 failed（9 条全是上述语义），Linux 同理；
产品主战场也是 Windows。哪天测试改成跨平台可跑，再挪到 ubuntu。

`rust-tests` 唯一跳过的是 `agent::chat::tests::run_shell_chinese_output_not_mojibake`：
它跑真 cmd 的 `chcp 65001 >nul & echo 中文测试OK`，断言中文不乱码——只有**中文 Windows**
（ACP=936）成立；GitHub 的 Windows runner 是 en-US（ACP=437），cmd 在写管道前就把中文变成 `?`，
解码器还没轮到。这是 runner locale 差异，不是产品回归：本地中文 Windows 仍要跑这条，
别把它从测试里删掉（2026-11-02 首跑实测：605 passed / 3 ignored / 1 failed 就是它）。

## 本地复现（与 CI 同口径）

```bash
npm install --global pnpm@9        # CI 用 pnpm 9；pnpm 11 会另外生成 pnpm-workspace.yaml 并拦 esbuild 构建脚本
pnpm install --frozen-lockfile
pnpm exec tsc --noEmit
pnpm build

cd src-tauri
cargo clippy --all-targets --message-format=json > ../clippy.json   # 有 warning 时退出码仍是 0
cd ..
node scripts/clippy-baseline.mjs check --input clippy.json            # 门禁：新增即失败
node scripts/clippy-baseline.mjs verify-shrink --base-ref origin/main # 断言 baseline 只减不增
cargo test --lib                    # 在 src-tauri 里跑：Windows 上 606 条可跑（605 过 + 1 条 locale 依赖）；macOS/Linux 有 9 条 Windows 语义用例必红
```

## clippy 存量账：本机 315 条 / CI 334 条（2026-11-02，rustc 1.93.1）

`cargo clippy -- -D warnings || cargo clippy` 时代这条闸门永远绿，warning 只进不出，
到 2026-11-02 已经欠了 **334 条**（`ubuntu-latest` 实测：lib 171 + lib test 163；
本机 macOS arm64 是 315 条 = lib 158 + lib test 157，`cfg` 差异让 Linux 多出一批 `dead_code`）。

直接改成 `cargo clippy --all-targets -- -D warnings` 会在一天内把 `rust` job 打成红灯，
而且没人能在一个 PR 里修完 334 条而不碰业务代码。所以这里**不追求一次归零，改成冻结存量**：

> **存量放行，新增即失败；baseline 只许变小，不许变大。**
> 硬截止 **2026-12-31**：到期前必须把 baseline 撤掉（B4），换回 `-D warnings`。

主要来源（本机口径）：

| 条数 | lint |
|---|---|
| 83 | `dead_code`（多是只有 Windows 调用点的函数/常量） |
| 52 | `clippy::let_unit_value` |
| 38 | `clippy::needless_borrows_for_generic_args` |
| 22 | `clippy::doc_lazy_continuation` |
| 12 | `clippy::doc_overindented_list_items` |
| 24 | `unused_mut` / `too_many_arguments` / `useless_asref` / `manual_div_ceil` / `needless_borrow` 等 |

## baseline 门禁怎么工作

三件东西：`clippy-baseline.json`（账本）、`scripts/clippy-baseline.mjs`（生成/比对）、
`clippy-baseline` job（CI 执行）。

### 指纹：lint 名 + 相对路径 + item 名，**不含行号**

```
clippy::let_unit_value|src-tauri/src/agent/chat.rs|fn exec_tool
dead_code|src-tauri/src/installer.rs|fn with_path
```

- **不加行号**：行号会随任何无关改动（改个 import、插一行注释）整体漂移。行号进指纹，
  baseline 就变成「每次 rebase 都要 `--update`」的噪音源，而 `--update` 一放开，门禁就等于没有。
- **item 名**取「包裹这条诊断的最内层 item」（`fn foo` / `const BAR` / `impl X` …），
  由脚本扫源码算出（先抹掉字符串与注释，再按花括号区间归位；doc 类诊断指向注释行，
  会改取它下面那个 item）。
- **`count`** 是该指纹出现的次数（同一处 warning 会在 lib 与 lib test 两次编译里各出现一次，
  所以典型值是 2）。**计数上涨也算新增**——同一函数里多犯一次同样的错，一样红。

**账本与诊断必须同源**：item 名是扫源码算出来的，所以 `check` 要拿**当前源码树**跑出来的 JSON
（改了代码就重跑 clippy；CI 天然满足）。拿旧 JSON 比会看到一串错位的「新增/减少」。
另外账本是在 **CI 的 ubuntu** 上生成的，本机 macOS 直接比对会看到平台专属差异（约 10 条 `dead_code`
等 `cfg` 差异），以 CI 为准。

### 门禁语义（CI 两条断言）

```yaml
- name: baseline 只减不增（对比 base 分支）
  run: node scripts/clippy-baseline.mjs verify-shrink --base-ref origin/<base>
- name: baseline 门禁（新增即失败，存量放行）
  run: node scripts/clippy-baseline.mjs check --input clippy.json
```

1. `check`：本次实测的指纹集合 ∖ baseline 必须为空，且任何指纹的 count 不得高于 baseline。
   多了就红，少了（清理推进）只提示、不拦。
2. `verify-shrink`：把 PR 里的 `clippy-baseline.json` 和 base 分支上的旧版本按 JSON 语义比，
   出现「新增条目 / 计数上涨 / 总数上涨」直接红。这等价于「`git diff` 里 baseline 只剩删除行」，
   但不会被重排、格式化或手改顺序绕过。**这条断言就是 `--update` 的唯一约束**。

### `--update`（`generate`）纪律

- `generate` 只在**清理 PR**（B1~B3）里手工执行一次，用来把掉下去的存量从账本里删掉；
- **CI 永不调用 `generate`**：门禁只读账本，从不写账本；
- 清理 PR 的 diff 里，`clippy-baseline.json` 只应有删除行；`verify-shrink` 会当场验证；
- 想靠 `--update` 把新 warning 记进账本 = 让 `verify-shrink` 红，走不通。

### 清理拆分顺序（多个小 PR，别和功能混）

- **B1 --fix 机械项**：`cargo clippy --all-targets --fix --allow-dirty` 吃掉
  `needless_borrows*` / `let_unit_value` / `unused_mut` / `useless_asref`，人工过一遍 diff；
- **B2 doc 缩进**：`doc_lazy_continuation` / `doc_overindented_list_items` 手改注释缩进；
- **B3 dead_code 补 cfg/allow**：Windows-only 调用点补 `#[cfg(windows)]` 或删掉真死代码。
  **不用 `#[allow(dead_code)]` 一把盖住**；确需 allow 的，**全仓上限 20 条，且每条必须带注释写明理由**
  （没注释的 allow 当作洗账，评审直接打回）；
- **B4 撤掉 baseline**：存量归零后删掉 `clippy-baseline.json`、`scripts/clippy-baseline.mjs`
  与本 job 的比对步骤，`rust` job 的 clippy 改回 `cargo clippy --all-targets -- -D warnings`。

每一步做完都在同一个 PR 里 `generate` 一次，让账本跟着掉——`verify-shrink` 只允许它变小。

### 新代码不小心引入了 warning 怎么办

1. 修掉（首选）；mechanical 的 `cargo clippy --fix` 就够；
2. 确实无法避免（平台相关、trait 签名约束），在**那一行**写 `#[allow(clippy::xxx)]` 并注明理由，
   计入上面的 20 条上限；
3. **不要** `generate` 一把抬高 baseline —— 那样 `verify-shrink` 会红，评审也会拒。

### 已知风险：`stable` 工具链漂移

job 用的是 `dtolnay/rust-toolchain@stable`，rustc 小版本升级可能带来新 lint，
诊断会凭空变多 → 门禁红，但代码一行没动。处理办法（按顺序）：

1. 先确认确实是工具链新增 lint（`rustc --version` 与本地/线上对比，日志里 lint 名是新的）；
2. 在**独立 PR** 里说明原因、`generate` 一次、把新条目连同「这是工具链新增」写进 PR 描述；
   该 PR 的 `verify-shrink` 会红——这是**预期的**，需要维护者在描述里显式认账后合并；
3. 想彻底避免漂移，可以把本 job 的工具链钉成具体版本（与 `rust` job 的 `stable` 分家），
   代价是升级要手动改数字。

## 预估时长（rust-cache 命中）

- `static-checks` ≈ 40s
- `frontend` ≈ 1.5 min（install 20s + tsc 10s + build 40s）
- `rust` ≈ 3~4 min（冷缓存 +2~3 min）
- `clippy-baseline` ≈ 3~4 min（`--all-targets` 比 `rust` job 的 `cargo clippy` 多编 test target）
- `rust-tests` ≈ 5 min（冷，2026-11-02 首跑实测 4m50s，其中测试本身 59s）
- 墙钟合计 ≈ 5~6 min，五个 job 并行
