# CI 门禁清单（2026-11-02 起）

`.github/workflows/ci.yml` 的每个 job 各守一段。本文说明每段"证明什么"、为什么按平台拆、
以及当前还欠的账（clippy 存量）。

## Job 地图

| job | runner | 跑什么 | 红意味着 |
|---|---|---|---|
| `static-checks` | ubuntu | 泄漏闸门 `scripts/check-leak.mjs`、版本号四处一致、关键修复防回退 grep | 敏感信息进仓、发版版本号没同步、历史 bug 被回退 |
| `frontend` | ubuntu | `pnpm install --frozen-lockfile` → `pnpm exec tsc --noEmit` → `pnpm build` | lockfile 与 package.json 漂移、TS 类型错、静态自检/vite 构建失败 |
| `rust` | ubuntu | `cargo check`、`cargo clippy`（只报错误不卡 warning）、action-parity 清单未漂移 | 编译失败 / 影核生成文件漂移 |
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
cargo clippy                        # 只报错误，不卡 warning（本 PR 未收紧）
cargo test --lib                    # Windows 上 606 条可跑（605 过 + 1 条 locale 依赖）；macOS/Linux 有 9 条 Windows 语义用例必红
```

## clippy 为什么还没上闸：本机 315 条 / CI 334 条（2026-11-02，rustc 1.93.1）

本 PR 只加了两道**新**闸门（前端、Rust 测试），**没有**动 clippy。
原因：`cargo clippy -- -D warnings || cargo clippy` 时代这条闸门永远绿，warning 只进不出，
到 2026-11-02 已经欠了 **334 条**（`ubuntu-latest` 首跑日志：lib 171 + lib test 163；
本机 macOS arm64 是 315 条 = lib 158 + lib test 157，`cfg` 差异让 Linux 多出一批 `dead_code`）。

直接把这一行改成 `cargo clippy --all-targets -- -D warnings` 会在一天内把 `rust` job 打成红灯，
而且没人能在一个 PR 里修完 334 条而不碰业务代码。所以本 PR **把 clippy 留在原样**，让 main 保持绿，
收紧这件事按下面的形状单独做（见「后续：baseline 冻结方案」）。

主要来源（本机口径）：

| 条数 | lint |
|---|---|
| 83 | `dead_code`（多是只有 Windows 调用点的函数/常量） |
| 52 | `clippy::let_unit_value` |
| 38 | `clippy::needless_borrows_for_generic_args` |
| 22 | `clippy::doc_lazy_continuation` |
| 12 | `clippy::doc_overindented_list_items` |
| 24 | `unused_mut` / `too_many_arguments` / `useless_asref` / `manual_div_ceil` / `needless_borrow` 等 |

## 后续：baseline 冻结方案（独立 PR，不在本 PR 里）

多模型会审结论：**不追求「一次归零」，改成「冻结存量、禁止新增」**，否则这道闸门永远上不去。
形状定稿如下，实现落在后续 PR（分支 `ci/clippy-baseline`）：

1. **baseline 文件**：把当前全部 warning 的稳定指纹落盘（`clippy-baseline.json`）。
   指纹 = **lint 名 + 相对路径 + item 名**，**不含行号**——行号会随任何无关改动漂移，
   一旦进指纹，baseline 就变成「每次 rebase 都要 --update」的噪音源。
2. **门禁语义**：CI 跑 clippy → 与 baseline 比对 → **新增 warning 直接失败，存量放行**。
   baseline 只允许减不允许增：生成脚本提供 `--update` 但 CI 不调用，
   且用 `git diff` 断言 baseline 的改动只能是删除条目。
3. **清理拆分顺序**（多个小 PR，别和功能混）：
   - **B1** `cargo clippy --all-targets --fix --allow-dirty` 吃掉机械项
     （`needless_borrows*`、`let_unit_value`、`unused_mut`、`useless_asref`），人工过 diff；
   - **B2** doc 类（`doc_lazy_continuation` / `doc_overindented_list_items`）手改注释缩进；
   - **B3** `dead_code`：Windows-only 调用点补 `#[cfg(windows)]` 或删掉真死代码；
     **不用 `#[allow(dead_code)]` 一把盖住**；确需 allow 的，**全仓上限 20 条，每条必须带注释说明理由**；
   - **B4** 存量归零后撤掉 baseline 与比对步骤，改回 `cargo clippy --all-targets -- -D warnings`。
4. **硬截止**：baseline **必须在 2026-12-31 前撤掉**（B4）。逾期未清完的，由维护者在
   `docs/ci-gates.md` 里显式续期并写明剩余条数，不允许静默续命。

## 预估时长（rust-cache 命中）

- `static-checks` ≈ 40s
- `frontend` ≈ 1.5 min（install 20s + tsc 10s + build 40s）
- `rust` ≈ 3~4 min（冷缓存 +2~3 min）
- `rust-tests` ≈ 5 min（冷，2026-11-02 首跑实测 4m50s，其中测试本身 59s）
- 墙钟合计 ≈ 5 min，四个 job 并行（首跑实测：static-checks 5s / frontend 45s / rust 2m42s / rust-tests 4m50s）
