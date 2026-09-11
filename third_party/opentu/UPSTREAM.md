# OpenTu 维护说明

U-King 长期维护的 fork 是 `https://github.com/dongsheng123132/opentu.git`（origin）；上游是 `https://github.com/ljquan/opentu.git`（upstream）。当前已发布组件 `opentu-1.1.6-uking.4` 仍锁定上游 v1.1.6 的 `48802871554c5b8221b4c5d70baff0b68d00df46`，提交信息为 `chore bump version 1.1.6`。它不是 fork 当前 HEAD `f57c21235d3b2e970131ed0a583a0f0f8aa2b054`，不得把 fork HEAD 标作这个已发布包的来源。

`lock.json` 是构建输入的唯一锁定记录。它锁定 pnpm 10.21.0、`pnpm-lock.yaml` 和 `LICENSE` 的 SHA-256，以及 `dist/apps/web` 输出目录。构建脚本会在应用 U-King 补丁前检查这些值和干净工作树；校验不通过时停止，不能用其他提交、重新生成的 lockfile 或 fork HEAD 代替。

U-King bridge 补丁独立保存在 `third_party/opentu/patches/`，构建时临时应用、结束时反向恢复。通用 OpenTu 修复应先向 `ljquan/opentu` 提 PR；仅 U-King 的项目桥接、安全边界和本地素材留在这里，避免把产品集成改动伪装成上游版本。

升级流程：先把目标上游 release commit 导入并在 fork 中标记，之后才可将 fork 作为新发布包的可追溯来源；从该精确提交重新计算 lock 中两个哈希并验证构建；生成新的 bundle ID、归档和完整性清单；最后在全部验证通过后，单独更新发布 catalog。绝不复用、覆盖或冒充既有 catalog 条目；`.3` 在 `.2` 的本地项目桥接基础上，新增了仅 U-King 宿主模式下的托管供应商提示；`.4` 在设置面板的最终渲染边界统一拦截 U-King 宿主模式，避免菜单、模型下拉或工具入口显示上游供应商配置。
