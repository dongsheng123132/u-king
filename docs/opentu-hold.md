# 创作画布暂存

创作画布当前不进入发行链。应用保留“待上线”入口，核心会拒绝组件安装和本地服务启动；已有项目文件、停止、卸载和导出路径保持可用。

当前已发布的可选组件是 `opentu-1.1.6-uking.7`。它由固定上游版本构建，自动构建脚本只应用 `third_party/opentu/patches/` 中的 `0001` 至 `0004` 补丁。这个已发布组件的字节不可因后续实验而改变。

`third_party/opentu/deferred/0005-uking-image-recovery-credentials.patch` 与 `0006-uking-generation-lifecycle.patch` 是未发布的恢复与生成生命周期实验补丁，未被构建脚本读取。对应的桥接回归脚本仍保留在 `scripts/test-opentu-generation-lifecycle.mjs`，只引用归档补丁。

归档补丁已完成固定上游补丁顺序检查和生命周期桥接的静态/逻辑验证。重新启用前仍需完成发布准入、端到端生成验收，以及针对恢复流程的完整组件构建验证。
