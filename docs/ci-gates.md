# CI 门禁清单（2026-11-02 起）

`.github/workflows/ci.yml` 的每个 job 各守一段。本文说明每段"证明什么"、为什么按平台拆、
以及当前还欠的账（clippy 存量）。

## Job 地图

| job | runner | 跑什么 | 红意味着 |
|---|---|---|---|
| `static-checks` | ubuntu | 泄漏闸门 `scripts/check-leak.mjs`、版本号四处一致、关键修复防回退 grep | 敏感信息进仓、发版版本号没同步、历史 bug 被回退 |
| `frontend` | ubuntu | `pnpm install --frozen-lockfile` → `pnpm exec tsc --noEmit` → `pnpm build` | lockfile 与 package.json 漂移、TS 类型错、静态自检/vite 构建失败 |
| `rust` | ubuntu | `cargo check`、`cargo clippy --all-targets -- -D warnings`、action-parity 清单未漂移 | 编译失败 / 有 clippy warning / 影核生成文件漂移 |
| `rust-tests` | windows | `cargo test --lib`（614 条 `#[test]`，全在 lib） | Rust 单元测试红 |

**为什么测试跑 Windows**：这批测试写的是 Windows 语义——`discover_tools_from` 在非 Windows
只找无扩展名文件、`unwrap_shim` 解的是 `.cmd` 壳、用例里直接写 `%APPDATA%\npm` 和
`C:\Users\...`。同一个 HEAD 在 macOS 上是 561 passed / 9 failed（9 条全是上述语义），Linux 同理；
产品主战场也是 Windows。哪天测试改成跨平台可跑，再挪到 ubuntu。

## 本地复现（与 CI 同口径）

```bash
npm install --global pnpm@9        # CI 用 pnpm 9；pnpm 11 会另外生成 pnpm-workspace.yaml 并拦 esbuild 构建脚本
pnpm install --frozen-lockfile
pnpm exec tsc --noEmit
pnpm build

cd src-tauri
cargo clippy --all-targets -- -D warnings
cargo test --lib                   # Windows 上全绿；macOS/Linux 会有上面那 9 条 Windows 语义用例
```

## clippy 存量：315 条（2026-11-02，macOS arm64 + rustc 1.93.1）

`cargo clippy -- -D warnings || cargo clippy` 时代这条闸门永远绿，warning 只进不出。现在一次结账：

| 条数 | 来源 |
|---|---|
| 158 | lib 单元（`-D warnings` 后直接编译失败） |
| 157 | lib test 单元（同一批，含测试代码） |
| 83 | `dead_code`（多是只有 Windows 调用点的函数/常量） |
| 52 | `clippy::let_unit_value` |
| 38 | `clippy::needless_borrows_for_generic_args` |
| 22 | `clippy::doc_lazy_continuation` |
| 12 | `clippy::doc_overindented_list_items` |
| 其余 | `unused_mut` / `too_many_arguments` / `useless_asref` / `manual_div_ceil` / `needless_borrow` … |

Linux runner 上的条数会因 `cfg` 差异略有出入，以 CI 日志为准。

清账顺序（建议独立 PR，别和功能混）：

1. `cargo clippy --all-targets --fix --allow-dirty` 吃掉机械项（`needless_borrows*`、`let_unit_value`、
   `unused_mut`、`useless_asref`），人工过一遍 diff；
2. doc 类（`doc_lazy_continuation` / `doc_overindented_list_items`）手改注释缩进；
3. `dead_code`：Windows-only 调用点补 `#[cfg(windows)]` 或删掉真死代码——**不要**用
   `#[allow(dead_code)]` 一把盖住；
4. 剩余 lint 手修。`--all-targets` 归零后这条闸门自然转绿。

在此之前 `rust` job 会红，这是**设计如此**：门禁不再用兜底假装通过。想先保 main 绿，
就把上面 4 步做成一个 PR 先合，再合门禁这条。

## 预估时长（rust-cache 命中）

- `static-checks` ≈ 40s
- `frontend` ≈ 1.5 min（install 20s + tsc 10s + build 40s）
- `rust` ≈ 3~4 min（冷缓存 +2~3 min）
- `rust-tests` ≈ 2~3 min（热）/ 8~10 min（冷；Windows 首次编译 Tauri 依赖最贵）
- 墙钟合计 ≈ 4~5 min（热）/ 10~12 min（冷），四个 job 并行
