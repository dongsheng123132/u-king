# 2origin kernel vendor 快照（只读）

- 上游：`dongsheng123132/2origin`
- 快照基线：`f96171c`（kernel 授权链完整性基线）
- 本地位置：`src/vendor/2origin-kernel/`
- 授权：Apache-2.0，完整许可证见同目录 `LICENSE`，来源为上游仓库根目录 `LICENSE`。

这三个运行时文件（`kernel.mjs`、`sig.mjs`、`clock.mjs`）是为保证 U-King 的 Tauri bundle 和干净 clone 可自包含构建而复制进来的上游快照。它们是**只读副本**：不要在此目录修改内核逻辑；升级时请从上游以新的 commit 整体替换并更新本说明。
