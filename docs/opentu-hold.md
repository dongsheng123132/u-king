# 创作画布暂存

> **2026-10-04 更新**：应用侧的创作画布（`creator_local` 后端、`CreatorCanvas.tsx`、`creator-components` 目录、
> 12 个 `runtime.creator.{canvas,component,project,image}.*` 动作与「待上线」子页）已按收敛方案删除，
> 取回方式见归档标签 `archive/pre-delete-2026-10-04`。下文只描述 `third_party/opentu` 的构建侧遗留，
> 其中关于“应用保留入口 / 核心拒绝组件安装”的句子已不再成立。

创作画布当前不进入发行链。（已删除，见上。）

当前已发布的可选组件是 `opentu-1.1.6-uking.7`。它由固定上游版本构建，自动构建脚本只应用 `third_party/opentu/patches/` 中的 `0001` 至 `0004` 补丁。这个已发布组件的字节不可因后续实验而改变。

`third_party/opentu/deferred/0005-uking-image-recovery-credentials.patch` 与 `0006-uking-generation-lifecycle.patch` 是未发布的恢复与生成生命周期实验补丁，未被构建脚本读取。对应的桥接回归脚本仍保留在 `scripts/test-opentu-generation-lifecycle.mjs`，只引用归档补丁。

归档补丁已完成固定上游补丁顺序检查和生命周期桥接的静态/逻辑验证。重新启用前仍需完成发布准入、端到端生成验收，以及针对恢复流程的完整组件构建验证。
