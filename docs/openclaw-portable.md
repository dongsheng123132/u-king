# OpenClaw Windows 绿色预览包

固定 Windows x64 完整包由 `scripts/build-openclaw-portable.mjs` 从显式 U-King exe 和已核验 runtime 缓存组装。`portable.json` 开启受管便携模式；钱包、日志和时间轴在包内 `U-King/data/uking`，OpenClaw 资料在 `U-King/OpenClaw`。

默认配置只做 schema/能力验证，不调用模型；显式 probe 动作才可能消耗额度。充值只打开浏览器页面，不创建支付。OpenClaw 2026.8.1 的 config CLI 已在 NTFS 与 exFAT 基线验证。构建期只补丁已实证失败的 workspace writer：受管包进程选择 fs-safe `verify-content-with-lock`，默认仍为 strict。每个包均附带哈希、许可证和补丁说明。
